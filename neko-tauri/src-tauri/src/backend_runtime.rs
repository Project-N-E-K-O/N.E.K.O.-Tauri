use crate::platform;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

pub(crate) struct BackendProcesses(Mutex<Vec<Child>>);

#[cfg(debug_assertions)]
fn runtime_dir(_app: &tauri::App) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("无法定位 N.E.K.O 源码目录")
}

#[cfg(not(debug_assertions))]
fn runtime_dir(app: &tauri::App) -> PathBuf {
    app.path().resource_dir().expect("无法获取资源目录")
}

#[cfg(target_os = "windows")]
fn python_bin(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(".venv").join("Scripts").join("python.exe")
}

#[cfg(target_os = "macos")]
fn python_bin(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(".venv").join("bin").join("python3")
}

#[cfg(target_os = "linux")]
fn python_bin(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(".venv").join("bin").join("python")
}

pub(crate) fn start(app: &tauri::App) -> Result<BackendProcesses, String> {
    let runtime_dir = runtime_dir(app);
    let python_bin = python_bin(&runtime_dir);
    if !python_bin.is_file() {
        return Err(format!(
            "Python 虚拟环境不存在或不可用: {}。请先在项目根目录执行 uv sync --frozen",
            python_bin.display()
        ));
    }

    let scripts = ["memory_server.py", "main_server.py", "agent_server.py"];
    let mut children = Vec::new();

    for script in scripts {
        let script_path = runtime_dir.join(script);
        if !script_path.is_file() {
            terminate_children(&mut children);
            return Err(format!("后端入口文件不存在: {}", script_path.display()));
        }

        println!("尝试启动: {script_path:?}");
        match Command::new(&python_bin)
            .arg(&script_path)
            .current_dir(&runtime_dir)
            .spawn()
        {
            Ok(child) => {
                println!("成功启动后端服务: {script}");
                children.push(child);
            }
            Err(error) => {
                terminate_children(&mut children);
                return Err(format!("启动后端服务失败 {script}: {error}"));
            }
        }
    }

    Ok(BackendProcesses(Mutex::new(children)))
}

pub(crate) fn wait_for_main_server(window: tauri::WebviewWindow) {
    let address: SocketAddr = "127.0.0.1:48911".parse().expect("主服务地址无效");

    thread::spawn(move || {
        for _ in 0..600 {
            if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
                platform::prepare_for_navigation(&window);
                let mut main_url = "http://127.0.0.1:48911".to_string();
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

pub(crate) fn terminate(state: &BackendProcesses) {
    let mut children = state.0.lock().unwrap();
    terminate_children(&mut children);
}

fn terminate_children(children: &mut [Child]) {
    for child in children {
        println!("正在关闭关联的 Python 进程: {}", child.id());
        let _ = child.kill();
    }
}
