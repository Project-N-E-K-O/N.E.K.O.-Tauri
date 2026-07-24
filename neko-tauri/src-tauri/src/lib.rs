mod backend_runtime;
mod platform;
mod radar;

use tauri::webview::PageLoadEvent;
use tauri::Manager;

fn is_backend_page(url: &tauri::Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        && url.port_or_known_default() == Some(48911)
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn set_click_through(
    window: tauri::Window,
    ignore: bool,
    reason: Option<String>,
    details: Option<String>,
) -> Result<(), String> {
    // Only structured hit-test metadata is accepted here. Never log DOM text
    // or conversation content, and cap input length to avoid terminal floods.
    let reason: String = reason
        .unwrap_or_else(|| "unspecified".to_string())
        .chars()
        .take(80)
        .collect();
    let details: String = details
        .unwrap_or_else(|| "{}".to_string())
        .chars()
        .take(32_768)
        .collect();
    let window_label = window.label().to_string();

    println!(
        "[ClickThrough] 请求: window={window_label} ignore={ignore} reason={reason} details_len={}",
        details.len()
    );

    platform::set_click_through(&window, ignore, &reason, &details).map_err(|error| {
        let message = format!(
            "[ClickThrough] 应用失败: window={window_label} ignore={ignore} reason={reason} error={error}"
        );
        println!("{message}");
        message
    })?;

    println!("[ClickThrough] 已提交: window={window_label} ignore={ignore} reason={reason}");
    Ok(())
}

#[tauri::command]
fn open_new_window(app: tauri::AppHandle, url: String, label: String) -> Result<(), String> {
    let parsed_url = url
        .parse()
        .map_err(|error| format!("设置面板 URL 无效: {error}"))?;
    platform::open_settings_window(&app, parsed_url, label)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    platform::prepare_process_environment();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .on_page_load(|webview, payload| {
            if platform::INJECT_CLICK_THROUGH_BRIDGE
                && webview.label() == "main"
                && payload.event() == PageLoadEvent::Finished
                && is_backend_page(payload.url())
            {
                if let Err(error) = webview.eval(include_str!("click_through.js")) {
                    eprintln!("注入 Tauri 鼠标命中桥接失败: {error}");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            set_click_through,
            open_new_window
        ])
        .setup(|app| {
            platform::configure_app(app);
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| "Tauri 主窗口不存在".to_string())?;

            platform::configure_main_window(app, &window)?;
            platform::startup_input_region(&window)?;

            let processes = backend_runtime::start(app)?;
            app.manage(processes);
            backend_runtime::wait_for_main_server(window.clone());
            radar::start(window);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    builder.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<backend_runtime::BackendProcesses>() {
                backend_runtime::terminate(&state);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::is_backend_page;

    #[test]
    fn injects_native_hit_test_only_into_the_backend_page() {
        let backend = tauri::Url::parse("http://127.0.0.1:48911/").unwrap();
        let backend_localhost = tauri::Url::parse("http://localhost:48911/chat").unwrap();
        let vite_startup = tauri::Url::parse("http://127.0.0.1:1420/").unwrap();
        let unrelated = tauri::Url::parse("https://example.com:48911/").unwrap();

        assert!(is_backend_page(&backend));
        assert!(is_backend_page(&backend_localhost));
        assert!(!is_backend_page(&vite_startup));
        assert!(!is_backend_page(&unrelated));
    }
}
