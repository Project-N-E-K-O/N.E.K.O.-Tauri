//! Native desktop integration boundary.
//!
//! The web UI and backend lifecycle are shared. Window levels, cursor
//! coordinates and click-through behavior are delegated to exactly one
//! platform implementation at compile time.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorSample {
    pub(crate) absolute: (f64, f64),
    pub(crate) relative: (f64, f64),
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod niri;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;
#[cfg(target_os = "windows")]
pub(crate) use windows::*;

pub(crate) fn uses_niri_satellites() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var("GDK_BACKEND").as_deref() == Ok("wayland")
            && std::env::var_os("NIRI_SOCKET").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

pub(crate) fn after_satellite_reveal(
    window: &tauri::WebviewWindow,
    geometry: (f64, f64, f64, f64),
) {
    #[cfg(target_os = "linux")]
    if uses_niri_satellites() && window.label() != "main" {
        let Ok(title) = window.title() else {
            return;
        };
        std::thread::spawn(move || {
            for _ in 0..20 {
                if niri::bounds(&title).is_ok() {
                    if let Err(error) = niri::place(
                        &title,
                        niri::Geometry {
                            x: geometry.0,
                            y: geometry.1,
                            width: geometry.2,
                            height: geometry.3,
                        },
                    ) {
                        eprintln!("Niri satellite placement failed: {error}");
                    }
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            eprintln!("Niri satellite did not become mapped within the placement deadline");
        });
    }
    let _ = (window, geometry);
}

pub(crate) fn compositor_bounds(
    window: &tauri::WebviewWindow,
) -> Result<Option<(f64, f64, f64, f64)>, String> {
    #[cfg(target_os = "linux")]
    if uses_niri_satellites() && window.label() != "main" {
        let bounds = niri::bounds(&window.title().map_err(|error| error.to_string())?)?;
        return Ok(Some((bounds.x, bounds.y, bounds.width, bounds.height)));
    }
    let _ = window;
    Ok(None)
}

pub(crate) fn place_compositor_window(
    window: &tauri::WebviewWindow,
    geometry: (f64, f64, f64, f64),
) -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    if uses_niri_satellites() && window.label() != "main" {
        niri::place(
            &window.title().map_err(|error| error.to_string())?,
            niri::Geometry {
                x: geometry.0,
                y: geometry.1,
                width: geometry.2,
                height: geometry.3,
            },
        )?;
        return Ok(true);
    }
    let _ = (window, geometry);
    Ok(false)
}

/// Satellite windows use ordinary native toplevels. They never inherit the Pet
/// carrier's layer-shell lifecycle or process-global mouse-ignore state.
pub(crate) fn configure_auxiliary_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return linux::configure_auxiliary_window_impl(window);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window;
        Ok(())
    }
}

pub(crate) fn set_auxiliary_click_through(
    window: &tauri::Window,
    ignore: bool,
    details: &str,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return linux::set_auxiliary_click_through_impl(window, ignore, details);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = details;
        window
            .set_ignore_cursor_events(ignore)
            .map_err(|error| error.to_string())
    }
}

pub(crate) fn open_external_url(url: &tauri::Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https" | "mailto") {
        return Err("Unsupported external URL".into());
    }
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    command
        .arg(url.as_str())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("N.E.K.O desktop currently supports Linux, macOS and Windows");
