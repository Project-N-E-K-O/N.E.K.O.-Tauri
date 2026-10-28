use crate::platform;
use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tauri::Manager;

const DEFAULT_MAIN_SERVER_PORT: u16 = 48911;
const BACKEND_EXECUTABLE_ENV: &str = "NEKO_BACKEND_EXECUTABLE";
const BACKEND_SOURCE_ENV: &str = "NEKO_BACKEND_DIR";

/// Same lookup as N.E.K.O's `_read_port_env`: `NEKO_MAIN_SERVER_PORT`, then `MAIN_SERVER_PORT`.
pub(crate) fn main_server_port() -> u16 {
    static PORT: OnceLock<u16> = OnceLock::new();
    *PORT.get_or_init(|| {
        ["NEKO_MAIN_SERVER_PORT", "MAIN_SERVER_PORT"]
            .iter()
            .find_map(|key| parse_port(&env::var(key).ok()?))
            .unwrap_or(DEFAULT_MAIN_SERVER_PORT)
    })
}

fn parse_port(raw: &str) -> Option<u16> {
    raw.trim().parse::<u16>().ok().filter(|port| *port != 0)
}

pub(crate) struct BackendProcess(Mutex<Option<Child>>);

struct BackendCommand {
    program: PathBuf,
    args: Vec<PathBuf>,
    working_dir: PathBuf,
}

fn main_server_is_ready() -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], main_server_port()));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(200)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }

    let mut response = String::new();
    if stream
        .take(16 * 1024)
        .read_to_string(&mut response)
        .is_err()
    {
        return false;
    }
    let Some((_, body)) = response.split_once("\r\n\r\n") else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|payload| {
            payload
                .get("app")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .as_deref()
        == Some("N.E.K.O")
}

fn bundled_executable_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "projectneko_server.exe"
    } else {
        "projectneko_server"
    }
}

fn python_bin(runtime_dir: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        runtime_dir.join(".venv").join("Scripts").join("python.exe")
    } else if cfg!(target_os = "macos") {
        runtime_dir.join(".venv").join("bin").join("python3")
    } else {
        runtime_dir.join(".venv").join("bin").join("python")
    }
}

fn executable_command(path: PathBuf) -> Result<BackendCommand, String> {
    if !path.is_file() {
        return Err(format!("N.E.K.O 后端产物不存在: {}", path.display()));
    }
    let working_dir = path
        .parent()
        .ok_or_else(|| format!("无法确定后端产物目录: {}", path.display()))?
        .to_path_buf();
    Ok(BackendCommand {
        program: path,
        args: Vec::new(),
        working_dir,
    })
}

fn source_command(runtime_dir: PathBuf) -> Result<BackendCommand, String> {
    let launcher = runtime_dir.join("launcher.py");
    let python = python_bin(&runtime_dir);
    if !launcher.is_file() {
        return Err(format!(
            "NEKO_BACKEND_DIR 不是有效的 N.E.K.O 源码目录，缺少 {}",
            launcher.display()
        ));
    }
    if !python.is_file() {
        return Err(format!(
            "N.E.K.O 虚拟环境不存在: {}。请在后端仓库执行 uv sync --frozen",
            python.display()
        ));
    }
    Ok(BackendCommand {
        program: python,
        args: vec![launcher],
        working_dir: runtime_dir,
    })
}

fn resolve_backend_command(app: &tauri::App) -> Result<BackendCommand, String> {
    if let Some(path) = env::var_os(BACKEND_EXECUTABLE_ENV) {
        return executable_command(PathBuf::from(path));
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join("backend").join(bundled_executable_name());
        if bundled.is_file() {
            return executable_command(bundled);
        }
    }

    if let Some(path) = env::var_os(BACKEND_SOURCE_ENV) {
        return source_command(PathBuf::from(path));
    }

    Err(format!(
        "未找到 N.E.K.O 后端。请先启动端口 {} 的后端，或设置 {BACKEND_SOURCE_ENV}（源码目录）/ {BACKEND_EXECUTABLE_ENV}（构建产物）",
        main_server_port()
    ))
}

pub(crate) fn start(app: &tauri::App) -> Result<BackendProcess, String> {
    if main_server_is_ready() {
        println!("[Backend] 已连接正在运行的 N.E.K.O 服务，不接管其进程生命周期");
        return Ok(BackendProcess(Mutex::new(None)));
    }

    let command = resolve_backend_command(app)?;
    println!(
        "[Backend] 启动外部 N.E.K.O 后端: program={} cwd={}",
        command.program.display(),
        command.working_dir.display()
    );

    let child = Command::new(&command.program)
        .args(&command.args)
        .current_dir(&command.working_dir)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .spawn()
        .map_err(|error| {
            format!(
                "启动 N.E.K.O 后端失败 {}: {error}",
                command.program.display()
            )
        })?;

    Ok(BackendProcess(Mutex::new(Some(child))))
}

pub(crate) fn wait_for_main_server(window: tauri::WebviewWindow) {
    thread::spawn(move || {
        for _ in 0..600 {
            if main_server_is_ready() {
                platform::prepare_for_navigation(&window);
                let mut main_url = format!("http://127.0.0.1:{}", main_server_port());
                if let Some(query) = platform::diagnostic_query() {
                    main_url.push_str("/?");
                    main_url.push_str(query);
                }

                if let Ok(url) = tauri::Url::parse(&main_url) {
                    if let Err(error) = window.navigate(url) {
                        eprintln!("主服务已就绪，但页面跳转失败: {error}");
                    }
                }
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }

        platform::prepare_for_navigation(&window);
        eprintln!("等待主服务超时，窗口保留在启动页面");
    });
}

pub(crate) fn terminate(state: &BackendProcess) {
    let mut child = state.0.lock().unwrap();
    if let Some(process) = child.as_mut() {
        println!(
            "[Backend] 正在关闭由 Tauri 启动的后端进程: {}",
            process.id()
        );
        let _ = process.kill();
        let _ = process.wait();
    }
    *child = None;
}

#[cfg(test)]
mod tests {
    use super::parse_port;

    #[test]
    fn parses_port_overrides_like_the_backend() {
        assert_eq!(parse_port("47911"), Some(47911));
        assert_eq!(parse_port(" 47911 "), Some(47911));
        assert_eq!(parse_port(""), None);
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("65536"), None);
        assert_eq!(parse_port("abc"), None);
    }
}
