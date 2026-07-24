use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use gtk::prelude::*;
use tauri::Manager;
use webkit2gtk::{
    DeviceInfoPermissionRequest, PermissionRequestExt, SettingsExt, UserMediaPermissionRequest,
    WebViewExt,
};

use super::CursorSample;

pub(crate) const INJECT_CLICK_THROUGH_BRIDGE: bool = false;

static INPUT_REGION_GENERATION: AtomicU64 = AtomicU64::new(0);

const STATIC_INPUT_REGION_ENV: &str = "NEKO_X11_STATIC_INPUT_REGION";
const STATIC_INPUT_REGION_DEFAULT: InputRectangle = InputRectangle {
    x: 24,
    y: 24,
    width: 320,
    height: 220,
};
const INPUT_SENTINEL: InputRectangle = InputRectangle {
    x: 0,
    y: 0,
    width: 1,
    height: 1,
};
const POINTER_HOT_ZONE_SIZE: i32 = 192;
const DRAG_HOT_ZONE_SIZE: i32 = 768;
const BACKEND_MEDIA_PORT: u16 = 48911;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InputRectangle {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[link(name = "Xext")]
extern "C" {
    fn XShapeGetRectangles(
        display: *mut x11::xlib::Display,
        window: x11::xlib::Window,
        kind: i32,
        count: *mut i32,
        ordering: *mut i32,
    ) -> *mut x11::xlib::XRectangle;
}

pub(crate) fn prepare_process_environment() {
    // The current proven Linux implementation uses an X11 ShapeInput region.
    // Keep that behavior isolated here so a native Wayland backend can later
    // be added behind this module without affecting macOS or Windows.
    std::env::set_var("GDK_BACKEND", "x11");
}

pub(crate) fn configure_app(_app: &mut tauri::App) {}

fn is_kde_desktop_value(value: &str) -> bool {
    value
        .split([':', ';', ','])
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .any(|token| {
            token == "kde"
                || token.starts_with("kde-")
                || token.contains("plasma")
                || token.ends_with("/kde")
        })
}

fn is_kde_desktop() -> bool {
    [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]
    .iter()
    .filter_map(|name| std::env::var(name).ok())
    .any(|value| is_kde_desktop_value(&value))
}

fn is_trusted_media_origin(uri: &str) -> bool {
    let Ok(url) = tauri::Url::parse(uri) else {
        return false;
    };

    url.scheme() == "http"
        && matches!(
            url.host_str(),
            Some("127.0.0.1" | "localhost" | "::1" | "[::1]")
        )
        && url.port_or_known_default() == Some(BACKEND_MEDIA_PORT)
}

fn configure_media_permissions(window: &tauri::WebviewWindow) -> Result<(), String> {
    window
        .with_webview(|platform_webview| {
            let webview = platform_webview.inner();

            if let Some(settings) = WebViewExt::settings(&webview) {
                settings.set_enable_media_stream(true);
                settings.set_enable_webrtc(true);
            }

            webview.connect_permission_request(|webview, request| {
                let is_media_request = request
                    .dynamic_cast_ref::<UserMediaPermissionRequest>()
                    .is_some()
                    || request
                        .dynamic_cast_ref::<DeviceInfoPermissionRequest>()
                        .is_some();

                if !is_media_request {
                    // Leave unrelated permissions to WebKitGTK's default policy.
                    return false;
                }

                let uri = webview.uri().unwrap_or_default();
                let trusted = is_trusted_media_origin(uri.as_str());
                let origin = tauri::Url::parse(uri.as_str())
                    .ok()
                    .and_then(|url| {
                        let host = url.host_str()?.to_string();
                        Some(format!(
                            "{}://{}:{}",
                            url.scheme(),
                            host,
                            url.port_or_known_default().unwrap_or_default()
                        ))
                    })
                    .unwrap_or_else(|| "unknown".to_string());

                if trusted {
                    request.allow();
                    println!("[MediaPermission] 已允许本地 N.E.K.O 媒体请求: origin={origin}");
                } else {
                    request.deny();
                    println!("[MediaPermission] 已拒绝非可信媒体请求: origin={origin}");
                }

                true
            });
        })
        .map_err(|error| format!("无法配置 Linux WebView 媒体权限: {error}"))
}

pub(crate) fn configure_main_window(
    _app: &tauri::App,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    configure_media_permissions(window)?;
    configure_overlay(window)?;
    if is_kde_desktop() {
        // KWin constrains even oversized NORMAL windows to _NET_WORKAREA.
        // FULLSCREEN is the supported EWMH state that covers the complete
        // physical output instead of the panel-reserved work area.
        window
            .set_fullscreen(true)
            .map_err(|error| format!("无法将 KDE 主窗口设为全屏覆盖层: {error}"))?;
        println!("[WindowBounds] KDE 全屏覆盖状态已请求");
    } else {
        resize_to_virtual_desktop(window)?;
    }
    window
        .set_always_on_top(true)
        .map_err(|error| format!("无法将 Linux 主窗口置顶: {error}"))
}

fn configure_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    let gtk_window = window.gtk_window().map_err(|error| error.to_string())?;

    if is_kde_desktop() {
        // KWin constrains DOCK windows to _NET_WORKAREA. On a desktop with
        // panels that turns a 2560x1600 overlay into (for example) 2560x1488,
        // producing a visible boundary and clipping avatars near the bottom.
        // NORMAL + keep_above preserves full physical-monitor bounds on KDE.
        gtk_window.set_type_hint(gdk::WindowTypeHint::Normal);
        println!("[WindowType] KDE/Plasma detected; using NORMAL full-screen overlay");
    } else {
        // GNOME/Mutter can leave a maximized transparent NORMAL window below
        // the active application even while _NET_WM_STATE_ABOVE is present.
        // A dock is semantically closer to this desktop overlay there.
        gtk_window.set_type_hint(gdk::WindowTypeHint::Dock);
        println!("[WindowType] non-KDE desktop detected; using DOCK overlay");
    }
    gtk_window.set_keep_above(true);
    gtk_window.stick();
    Ok(())
}

fn resize_to_virtual_desktop(window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitors = window
        .available_monitors()
        .map_err(|error| error.to_string())?;
    let Some(first) = monitors.first() else {
        return Ok(());
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

    window
        .set_position(tauri::PhysicalPosition::new(min_x, min_y))
        .map_err(|error| error.to_string())?;
    window
        .set_size(tauri::PhysicalSize::new(
            (max_x - min_x) as u32,
            (max_y - min_y) as u32,
        ))
        .map_err(|error| error.to_string())
}

fn static_input_region_diagnostic() -> Option<InputRectangle> {
    let raw = std::env::var(STATIC_INPUT_REGION_ENV).ok()?;
    let normalized = raw.trim();

    if normalized.eq_ignore_ascii_case("1") || normalized.eq_ignore_ascii_case("true") {
        return Some(STATIC_INPUT_REGION_DEFAULT);
    }

    let values = normalized
        .split(',')
        .map(str::trim)
        .map(str::parse::<i32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    if values.len() != 4 || values[2] <= 0 || values[3] <= 0 {
        println!(
            "[InputRegionDiagnostic] 环境变量格式无效: {STATIC_INPUT_REGION_ENV}={normalized:?}; 应为 1 或 x,y,width,height"
        );
        return None;
    }

    Some(InputRectangle {
        x: values[0],
        y: values[1],
        width: values[2],
        height: values[3],
    })
}

fn query_x11_input_shape(gdk_window: &gdk::Window) -> Result<Vec<InputRectangle>, String> {
    const SHAPE_INPUT: i32 = 2;

    let x11_window = gdk_window
        .clone()
        .downcast::<gdkx11::X11Window>()
        .map_err(|_| "当前 GDK 窗口不是 X11 窗口".to_string())?;
    let display = unsafe { gdkx11::ffi::gdk_x11_get_default_xdisplay() };
    if display.is_null() {
        return Err("无法取得 X11 Display".to_string());
    }

    let mut count = 0;
    let mut ordering = 0;
    let rectangles = unsafe {
        XShapeGetRectangles(
            display,
            x11_window.xid(),
            SHAPE_INPUT,
            &mut count,
            &mut ordering,
        )
    };
    if rectangles.is_null() {
        return Err("XShapeGetRectangles 未返回输入区域".to_string());
    }

    let result = unsafe {
        std::slice::from_raw_parts(rectangles, count.max(0) as usize)
            .iter()
            .map(|rectangle| InputRectangle {
                x: i32::from(rectangle.x),
                y: i32::from(rectangle.y),
                width: i32::from(rectangle.width),
                height: i32::from(rectangle.height),
            })
            .collect()
    };
    unsafe {
        x11::xlib::XFree(rectangles.cast::<c_void>());
    }
    Ok(result)
}

fn apply_static_input_region(
    gtk_window: &gtk::ApplicationWindow,
    rectangle: InputRectangle,
) -> Result<Vec<InputRectangle>, String> {
    let gdk_window = gtk_window
        .window()
        .ok_or_else(|| "GTK 主窗口尚未创建 GDK 窗口".to_string())?;
    let region = gdk::cairo::Region::create_rectangle(&gdk::cairo::RectangleInt::new(
        rectangle.x,
        rectangle.y,
        rectangle.width,
        rectangle.height,
    ));

    gdk_window.input_shape_combine_region(&region, 0, 0);
    gdk_window.display().flush();
    gdk_window.display().sync();
    query_x11_input_shape(&gdk_window)
}

fn json_number(value: &serde_json::Value, path: &[&str]) -> Option<f64> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_f64().filter(|number| number.is_finite())
}

fn css_input_rectangle(
    values: &[serde_json::Value],
    dpr: f64,
    padding_css: f64,
) -> Option<InputRectangle> {
    if values.len() != 4 {
        return None;
    }
    let values = values
        .iter()
        .map(serde_json::Value::as_f64)
        .collect::<Option<Vec<_>>>()?;
    let width = values[2];
    let height = values[3];
    if !values.iter().all(|value| value.is_finite()) || width <= 0.0 || height <= 0.0 {
        return None;
    }

    let padding = (padding_css * dpr).ceil();
    Some(InputRectangle {
        x: (values[0] * dpr - padding).floor() as i32,
        y: (values[1] * dpr - padding).floor() as i32,
        width: (width * dpr + padding * 2.0).ceil() as i32,
        height: (height * dpr + padding * 2.0).ceil() as i32,
    })
}

fn pointer_input_rectangles(ignore: bool, reason: &str, details: &str) -> Vec<InputRectangle> {
    let mut rectangles = vec![INPUT_SENTINEL];
    let Ok(details): Result<serde_json::Value, _> = serde_json::from_str(details) else {
        return rectangles;
    };
    let dpr = json_number(&details, &["dpr"])
        .filter(|value| (0.5..=8.0).contains(value))
        .unwrap_or(1.0);

    if let Some(persistent_rectangles) = details
        .get("persistentRects")
        .and_then(serde_json::Value::as_array)
    {
        rectangles.extend(
            persistent_rectangles
                .iter()
                .filter_map(serde_json::Value::as_array)
                .filter_map(|values| css_input_rectangle(values, dpr, 2.0))
                .take(128),
        );
    }
    if ignore {
        return rectangles;
    }

    let dom_rect = details
        .get("domHit")
        .and_then(|value| value.get("rect"))
        .and_then(serde_json::Value::as_array);
    if let Some(rect) = dom_rect.filter(|rect| rect.len() == 4) {
        if let Some(rectangle) = css_input_rectangle(rect, dpr, 12.0) {
            rectangles.push(rectangle);
            return rectangles;
        }
    }

    let physical = details
        .get("physical")
        .and_then(serde_json::Value::as_array);
    if let Some(position) = physical.filter(|position| position.len() >= 2) {
        let x = position[0].as_f64();
        let y = position[1].as_f64();
        if let (Some(x), Some(y)) = (x, y) {
            if x.is_finite() && y.is_finite() {
                let size = if reason.starts_with("drag-active") {
                    DRAG_HOT_ZONE_SIZE
                } else {
                    POINTER_HOT_ZONE_SIZE
                };
                rectangles.push(InputRectangle {
                    x: x.round() as i32 - size / 2,
                    y: y.round() as i32 - size / 2,
                    width: size,
                    height: size,
                });
            }
        }
    }
    rectangles
}

fn apply_input_rectangles(
    gtk_window: &gtk::ApplicationWindow,
    rectangles: &[InputRectangle],
) -> Result<(i32, i32, Vec<InputRectangle>), String> {
    let gdk_window = gtk_window
        .window()
        .ok_or_else(|| "GTK 主窗口尚未创建 GDK 窗口".to_string())?;
    let width = gdk_window.width();
    let height = gdk_window.height();
    let mut clipped = Vec::with_capacity(rectangles.len());

    for rectangle in rectangles {
        let left = rectangle.x.max(0).min(width);
        let top = rectangle.y.max(0).min(height);
        let right = rectangle
            .x
            .saturating_add(rectangle.width)
            .max(0)
            .min(width);
        let bottom = rectangle
            .y
            .saturating_add(rectangle.height)
            .max(0)
            .min(height);
        if right > left && bottom > top {
            clipped.push(InputRectangle {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            });
        }
    }

    if clipped.is_empty() {
        clipped.push(INPUT_SENTINEL);
    }
    let cairo_rectangles: Vec<_> = clipped
        .iter()
        .map(|rectangle| {
            gdk::cairo::RectangleInt::new(
                rectangle.x,
                rectangle.y,
                rectangle.width,
                rectangle.height,
            )
        })
        .collect();
    let region = gdk::cairo::Region::create_rectangles(&cairo_rectangles);
    gdk_window.input_shape_combine_region(&region, 0, 0);
    gdk_window.display().flush();

    Ok((width, height, clipped))
}

pub(crate) fn startup_input_region(window: &tauri::WebviewWindow) -> Result<(), String> {
    let gtk_window = window.gtk_window().map_err(|error| error.to_string())?;
    if !gtk_window.is_realized() {
        // The main window is intentionally created hidden so KWin never
        // presents WebKitGTK's unpainted fullscreen surface. Realizing creates
        // the backing GDK window needed by XShape without mapping it onscreen.
        gtk_window.realize();
        println!("[WindowVisibility] Linux 隐藏主窗口已完成 GTK realize");
    }
    if let Some(rectangle) = static_input_region_diagnostic() {
        let actual = apply_static_input_region(&gtk_window, rectangle)?;
        println!(
            "[InputRegionDiagnostic] 启动时固定区域已核验: requested={rectangle:?} actual={actual:?}"
        );
    } else {
        let (width, height, regions) = apply_input_rectangles(&gtk_window, &[INPUT_SENTINEL])?;
        println!(
            "[ClickThrough] Linux 启动哨兵区域已应用: regions={regions:?} window_size={width}x{height}"
        );
    }
    Ok(())
}

pub(crate) fn set_click_through(
    window: &tauri::Window,
    ignore: bool,
    reason: &str,
    details: &str,
) -> Result<(), String> {
    if let Some(rectangle) = static_input_region_diagnostic() {
        let gtk_window_handle = window.clone();
        let reason = reason.to_string();
        window
            .run_on_main_thread(move || {
                match gtk_window_handle
                    .gtk_window()
                    .map_err(|error| error.to_string())
                    .and_then(|gtk_window| apply_static_input_region(&gtk_window, rectangle))
                {
                    Ok(actual) => println!(
                        "[InputRegionDiagnostic] 固定区域已核验: requested={rectangle:?} actual={actual:?}; 已忽略动态请求 ignore={ignore} reason={reason}"
                    ),
                    Err(error) => println!(
                        "[InputRegionDiagnostic] 固定区域应用或核验失败: requested={rectangle:?} error={error}"
                    ),
                }
            })
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let rectangles = pointer_input_rectangles(ignore, reason, details);
    let region_reason = reason.to_string();
    let generation = INPUT_REGION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let gtk_window_handle = window.clone();

    window
        .run_on_main_thread(move || {
            if INPUT_REGION_GENERATION.load(Ordering::SeqCst) != generation {
                println!(
                    "[ClickThrough] Linux 输入区域任务已丢弃: generation={generation} ignore={ignore}"
                );
                return;
            }

            let result = gtk_window_handle
                .gtk_window()
                .map_err(|error| error.to_string())
                .and_then(|gtk_window| apply_input_rectangles(&gtk_window, &rectangles));

            match result {
                Ok((width, height, applied)) => {
                    let preview_len = applied.len().min(12);
                    println!(
                        "[ClickThrough] Linux 局部输入区域已应用: generation={generation} ignore={ignore} reason={region_reason} region_count={} region_preview={:?} window_size={width}x{height}",
                        applied.len(),
                        &applied[..preview_len]
                    );
                }
                Err(error) => println!(
                    "[ClickThrough] Linux 输入区域应用失败: generation={generation} ignore={ignore} error={error}"
                ),
            }
        })
        .map_err(|error| error.to_string())
}

pub(crate) fn prepare_for_navigation(_window: &tauri::WebviewWindow) {
    // Do not call TAO's whole-window restore here. It would overwrite the
    // persistent X Shape region installed for Linux.
}

pub(crate) fn diagnostic_query() -> Option<&'static str> {
    static_input_region_diagnostic().map(|_| "neko_x11_static_input_region=1")
}

pub(crate) fn cursor_sample(window: &tauri::WebviewWindow) -> Option<CursorSample> {
    let cursor = window.cursor_position().ok()?;
    let origin = window.inner_position().ok()?;
    Some(CursorSample {
        absolute: (cursor.x, cursor.y),
        relative: (
            cursor.x - f64::from(origin.x),
            cursor.y - f64::from(origin.y),
        ),
    })
}

pub(crate) fn open_settings_window(
    app: &tauri::AppHandle,
    url: tauri::Url,
    label: String,
) -> Result<(), String> {
    let builder = tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::External(url))
        .title("设置面板")
        .inner_size(900.0, 700.0)
        .transparent(false)
        .decorations(true)
        .always_on_top(true);

    let builder = if let Some(parent) = app.get_webview_window("main") {
        builder
            .transient_for(&parent)
            .map_err(|error| format!("无法将设置面板关联到主覆盖层: {error}"))?
    } else {
        builder
    };

    let window = builder.build().map_err(|error| error.to_string())?;
    configure_media_permissions(&window)
}

#[cfg(test)]
mod tests {
    use super::{
        is_kde_desktop_value, is_trusted_media_origin, pointer_input_rectangles,
        static_input_region_diagnostic, InputRectangle, INPUT_SENTINEL,
        STATIC_INPUT_REGION_DEFAULT, STATIC_INPUT_REGION_ENV,
    };

    #[test]
    fn recognizes_kde_and_plasma_desktop_identifiers() {
        for value in [
            "KDE",
            "KDE:Plasma",
            "plasma",
            "plasmawayland",
            "/usr/share/xsessions/plasma",
        ] {
            assert!(is_kde_desktop_value(value), "{value}");
        }

        for value in ["GNOME", "ubuntu:GNOME", "X-Cinnamon", "XFCE", ""] {
            assert!(!is_kde_desktop_value(value), "{value}");
        }
    }

    #[test]
    fn allows_media_only_from_the_local_backend_origin() {
        assert!(is_trusted_media_origin("http://127.0.0.1:48911/"));
        assert!(is_trusted_media_origin(
            "http://localhost:48911/settings?tab=microphone"
        ));
        assert!(is_trusted_media_origin("http://[::1]:48911/"));

        assert!(!is_trusted_media_origin("http://127.0.0.1:1420/"));
        assert!(!is_trusted_media_origin("https://127.0.0.1:48911/"));
        assert!(!is_trusted_media_origin("http://example.com:48911/"));
        assert!(!is_trusted_media_origin(
            "http://127.0.0.1.evil.example:48911/"
        ));
        assert!(!is_trusted_media_origin("not a url"));
    }

    #[test]
    fn parses_static_input_region_diagnostic_configuration() {
        let previous = std::env::var_os(STATIC_INPUT_REGION_ENV);

        std::env::set_var(STATIC_INPUT_REGION_ENV, "1");
        assert_eq!(
            static_input_region_diagnostic(),
            Some(STATIC_INPUT_REGION_DEFAULT)
        );

        std::env::set_var(STATIC_INPUT_REGION_ENV, "10,20,300,400");
        assert_eq!(
            static_input_region_diagnostic(),
            Some(InputRectangle {
                x: 10,
                y: 20,
                width: 300,
                height: 400,
            })
        );

        std::env::set_var(STATIC_INPUT_REGION_ENV, "10,20,0,400");
        assert_eq!(static_input_region_diagnostic(), None);

        match previous {
            Some(value) => std::env::set_var(STATIC_INPUT_REGION_ENV, value),
            None => std::env::remove_var(STATIC_INPUT_REGION_ENV),
        }
    }

    #[test]
    fn keeps_only_the_sentinel_for_transparent_pixels() {
        assert_eq!(
            pointer_input_rectangles(true, "transparent", r#"{"physical":[339,189],"dpr":1.25}"#),
            vec![INPUT_SENTINEL]
        );
    }

    #[test]
    fn keeps_persistent_ui_regions_over_transparency() {
        assert_eq!(
            pointer_input_rectangles(
                true,
                "transparent",
                r#"{"dpr":1.25,"persistentRects":[[24,24,320,220],[100,500,40,20]]}"#
            ),
            vec![
                INPUT_SENTINEL,
                InputRectangle {
                    x: 27,
                    y: 27,
                    width: 406,
                    height: 281,
                },
                InputRectangle {
                    x: 122,
                    y: 622,
                    width: 56,
                    height: 31,
                },
            ]
        );
    }

    #[test]
    fn converts_dom_css_bounds_to_x11_physical_coordinates() {
        assert_eq!(
            pointer_input_rectangles(
                false,
                "dom-hit",
                r#"{"physical":[339,189],"dpr":1.25,"domHit":{"rect":[24,24,320,220]}}"#
            ),
            vec![
                INPUT_SENTINEL,
                InputRectangle {
                    x: 15,
                    y: 15,
                    width: 430,
                    height: 305,
                }
            ]
        );
    }

    #[test]
    fn creates_pointer_and_drag_hot_zones() {
        assert_eq!(
            pointer_input_rectangles(
                false,
                "live2d-alpha",
                r#"{"physical":[339,189],"dpr":1.25}"#
            )[1],
            InputRectangle {
                x: 243,
                y: 93,
                width: 192,
                height: 192,
            }
        );
        assert_eq!(
            pointer_input_rectangles(false, "drag-active", r#"{"physical":[339,189],"dpr":1.25}"#)
                [1],
            InputRectangle {
                x: -45,
                y: -195,
                width: 768,
                height: 768,
            }
        );
    }
}
