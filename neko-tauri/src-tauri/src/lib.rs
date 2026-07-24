use std::process::{Child, Command};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::{Emitter, Manager};

#[cfg(target_os = "macos")]
use cocoa::base::{id, nil, NO, YES};
#[cfg(target_os = "macos")]
use cocoa::foundation::{NSPoint, NSRect, NSSize};
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(target_os = "macos")]
static WINDOW_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "macos")]
static CLICK_THROUGH: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
const NEKO_INTERACTIVE_WINDOW_LEVEL: i32 = 8;

struct BackendProcesses(Mutex<Vec<Child>>);

#[derive(Debug, Clone, Copy, PartialEq)]
struct DesktopBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn validated_desktop_bounds(frame: DesktopBounds) -> Option<DesktopBounds> {
    (frame.x.is_finite()
        && frame.y.is_finite()
        && frame.width.is_finite()
        && frame.height.is_finite()
        && frame.width > 0.0
        && frame.height > 0.0)
        .then_some(frame)
}

#[cfg(target_os = "macos")]
unsafe fn cocoa_primary_screen_frame() -> Option<DesktopBounds> {
    // NSScreen frames are already logical Cocoa points. Keeping this prototype
    // on the primary screen avoids forcing one WebView across mixed-DPI displays,
    // which causes Tao/AppKit to restore Tauri's initial window frame.
    // `mainScreen` follows the current key window and can drift between
    // displays; screens[0] is the display that owns the menu bar.
    let screens: id = msg_send![class!(NSScreen), screens];
    if screens == nil {
        return None;
    }
    let count: usize = msg_send![screens, count];
    if count == 0 {
        return None;
    }
    let screen: id = msg_send![screens, objectAtIndex: 0_usize];

    let frame: NSRect = msg_send![screen, frame];
    validated_desktop_bounds(DesktopBounds {
        x: frame.origin.x,
        y: frame.origin.y,
        width: frame.size.width,
        height: frame.size.height,
    })
}

#[cfg(target_os = "macos")]
unsafe fn apply_cocoa_primary_screen_frame(ns_window: id) {
    let Some(frame) = cocoa_primary_screen_frame() else {
        eprintln!("没有可用的 macOS 主显示器布局");
        return;
    };

    println!(
        "应用 macOS 主屏窗口边界: x={}, y={}, width={}, height={}",
        frame.x, frame.y, frame.width, frame.height
    );
    let native_frame = NSRect::new(
        NSPoint::new(frame.x, frame.y),
        NSSize::new(frame.width, frame.height),
    );
    let () = msg_send![ns_window, setFrame: native_frame display: YES];
}

#[cfg(not(target_os = "macos"))]
fn resize_to_virtual_desktop(window: &tauri::WebviewWindow) -> Option<DesktopBounds> {
    let Ok(monitors) = window.available_monitors() else {
        return None;
    };
    let Some(first) = monitors.first() else {
        return None;
    };

    let first_position = first.position();
    let first_size = first.size();
    let mut min_x = first_position.x;
    let mut min_y = first_position.y;
    let mut max_x = first_position.x + first_size.width as i32;
    let mut max_y = first_position.y + first_size.height as i32;

    for monitor in &monitors[1..] {
        let position = monitor.position();
        let size = monitor.size();
        min_x = min_x.min(position.x);
        min_y = min_y.min(position.y);
        max_x = max_x.max(position.x + size.width as i32);
        max_y = max_y.max(position.y + size.height as i32);
    }

    let _ = window.unmaximize();
    let _ = window.set_position(tauri::PhysicalPosition::new(min_x, min_y));
    let _ = window.set_size(tauri::PhysicalSize::new(
        (max_x - min_x) as u32,
        (max_y - min_y) as u32,
    ));
    None
}

fn cursor_position_in_webview(window: &tauri::WebviewWindow) -> Option<(f64, f64)> {
    let cursor = window.cursor_position().ok()?;
    let window_position = window.inner_position().ok()?;
    let window_scale = window.scale_factor().ok()?;

    #[cfg(target_os = "macos")]
    {
        // tao reports the global cursor position using the primary monitor's
        // scale, while the window position uses the window's current scale.
        // Normalize both to shared logical points before subtracting.
        let cursor_scale = window
            .primary_monitor()
            .ok()
            .flatten()
            .map(|monitor| monitor.scale_factor())
            .filter(|scale| scale.is_finite() && *scale > 0.0)
            .unwrap_or(window_scale);
        let cursor_logical = cursor.to_logical::<f64>(cursor_scale);
        let window_logical = window_position.to_logical::<f64>(window_scale);
        Some((
            cursor_logical.x - window_logical.x,
            cursor_logical.y - window_logical.y,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Some((
            (cursor.x - window_position.x as f64) / window_scale,
            (cursor.y - window_position.y as f64) / window_scale,
        ))
    }
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn set_click_through(window: tauri::Window, ignore: bool) {
    #[cfg(target_os = "macos")]
    {
        CLICK_THROUGH.store(ignore, Ordering::SeqCst);
        let ptr = WINDOW_PTR.load(Ordering::SeqCst);
        if ptr != 0 {
            let ns_window = ptr as id;
            unsafe {
                let val = if ignore { YES } else { NO };
                let () = msg_send![ns_window, setIgnoresMouseEvents: val];
            }
        } else {
            let _ = window.set_ignore_cursor_events(ignore);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window.set_ignore_cursor_events(ignore);
    }
}

#[tauri::command]
fn open_new_window(app: tauri::AppHandle, url: String, label: String) {
    if let Ok(parsed_url) = url.parse() {
        let _ =
            tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::External(parsed_url))
                .title("设置面板")
                .inner_size(900.0, 700.0)
                .transparent(false)
                .decorations(true)
                .build();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .on_page_load(|webview, payload| {
            if webview.label() == "main" && payload.event() == PageLoadEvent::Finished {
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
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let resource_dir = app.path().resource_dir().expect("无法获取资源目录");

            #[cfg(target_os = "macos")]
            let python_bin = resource_dir.join(".venv").join("bin").join("python3");
            #[cfg(target_os = "windows")]
            let python_bin = resource_dir
                .join(".venv")
                .join("Scripts")
                .join("python.exe");

            let scripts = vec!["memory_server.py", "main_server.py", "agent_server.py"];
            let mut children = Vec::new();

            for script in scripts {
                let script_path = resource_dir.join(script);
                println!("尝试启动: {:?}", script_path);

                match Command::new(&python_bin)
                    .arg(&script_path)
                    .current_dir(&resource_dir)
                    .spawn()
                {
                    Ok(child) => {
                        println!("成功启动后端服务: {}", script);
                        children.push(child);
                    }
                    Err(e) => eprintln!("启动后端服务失败 {}: {}", script, e),
                }
            }

            app.manage(BackendProcesses(Mutex::new(children)));

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let window = app.get_webview_window("main").unwrap();

            #[cfg(target_os = "macos")]
            {
                let app_handle = app.handle().clone();
                let window_clone = window.clone();

                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(500));

                    let _ = app_handle.run_on_main_thread(move || {
                        if let Ok(ns_window_ptr) = window_clone.ns_window() {
                            let ns_window = ns_window_ptr as id;
                            unsafe {
                                WINDOW_PTR.store(ns_window as usize, Ordering::SeqCst);

                                let () = msg_send![ns_window, setCollectionBehavior: 273_u64];
                                // Keep the interactive window below the IME candidate
                                // panel. The previous cursor-level value
                                // (2_147_483_630) covered system input UI.
                                let () =
                                    msg_send![ns_window, setLevel: NEKO_INTERACTIVE_WINDOW_LEVEL];

                                let () = msg_send![ns_window, orderOut: nil];
                                let () = msg_send![ns_window, orderFrontRegardless];

                                // Reordering can restore Tauri's original frame, so the
                                // final geometry write must happen after it is visible.
                                apply_cocoa_primary_screen_frame(ns_window);

                                let ignore = CLICK_THROUGH.load(Ordering::SeqCst);
                                let val = if ignore {
                                    cocoa::base::YES
                                } else {
                                    cocoa::base::NO
                                };
                                let () = msg_send![ns_window, setIgnoresMouseEvents: val];
                            }
                        }
                    });
                });
            }

            #[cfg(not(target_os = "macos"))]
            let _ = resize_to_virtual_desktop(&window);

            let window_clone = window.clone();
            thread::spawn(move || loop {
                if let Some((x, y)) = cursor_position_in_webview(&window_clone) {
                    let _ = window_clone.emit("neko_radar_tick", (x, y));
                }
                thread::sleep(Duration::from_millis(16));
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    builder.run(|app_handle, event| match event {
        tauri::RunEvent::Exit => {
            let state: tauri::State<BackendProcesses> = app_handle.state();
            let mut children = state.0.lock().unwrap();
            for child in children.iter_mut() {
                println!("正在关闭关联的 Python 进程: {}", child.id());
                let _ = child.kill();
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::{validated_desktop_bounds, DesktopBounds};

    #[test]
    fn valid_main_screen_frame_is_preserved() {
        let frame = DesktopBounds {
            x: 0.0,
            y: 0.0,
            width: 2560.0,
            height: 1440.0,
        };

        assert_eq!(validated_desktop_bounds(frame), Some(frame));
    }

    #[test]
    fn invalid_main_screen_frame_is_rejected() {
        assert_eq!(
            validated_desktop_bounds(DesktopBounds {
                x: 0.0,
                y: 0.0,
                width: f64::NAN,
                height: 100.0,
            }),
            None
        );
    }
}
