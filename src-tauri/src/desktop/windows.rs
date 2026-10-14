use super::{geometry::Bounds, role::WindowRole, DesktopHost};
use crate::platform;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Display {
    pub(crate) id: usize,
    pub(crate) bounds: Bounds,
    pub(crate) work_area: Bounds,
    pub(crate) scale_factor: f64,
}

pub(crate) fn displays(window: &WebviewWindow) -> Result<Vec<Display>, String> {
    let monitors = window
        .available_monitors()
        .map_err(|error| error.to_string())?;
    let mut monitors = monitors;
    if let Some(primary) = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
    {
        monitors.sort_by_key(|monitor| monitor.position() != primary.position());
    }
    Ok(monitors
        .iter()
        .enumerate()
        .map(|(id, monitor)| {
            let scale = monitor.scale_factor();
            let position = monitor.position();
            let size = monitor.size();
            let work = monitor.work_area();
            Display {
                id,
                bounds: Bounds {
                    x: f64::from(position.x) / scale,
                    y: f64::from(position.y) / scale,
                    width: f64::from(size.width) / scale,
                    height: f64::from(size.height) / scale,
                },
                work_area: Bounds {
                    x: f64::from(work.position.x) / scale,
                    y: f64::from(work.position.y) / scale,
                    width: f64::from(work.size.width) / scale,
                    height: f64::from(work.size.height) / scale,
                },
                scale_factor: scale,
            }
        })
        .collect())
}

pub(crate) fn bounds(window: &WebviewWindow) -> Result<Bounds, String> {
    if platform::uses_niri_satellites()
        && window.label() != "main"
        && !window.is_visible().unwrap_or(false)
    {
        if let Some(bounds) = window
            .app_handle()
            .state::<DesktopHost>()
            .lock()?
            .windows
            .get(window.label())
            .and_then(|record| record.initial_bounds)
        {
            return Ok(bounds);
        }
    }
    if let Some((x, y, width, height)) = platform::compositor_bounds(window)? {
        return Ok(Bounds {
            x,
            y,
            width,
            height,
        });
    }
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let size = window.inner_size().map_err(|error| error.to_string())?;
    Ok(Bounds {
        x: f64::from(position.x) / scale,
        y: f64::from(position.y) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
    })
}

pub(crate) fn set_bounds(window: &WebviewWindow, value: Bounds) -> Result<(), String> {
    if !value.valid() {
        return Err("Invalid window bounds".into());
    }
    if platform::uses_niri_satellites()
        && window.label() != "main"
        && !window.is_visible().unwrap_or(false)
    {
        if let Some(record) = window
            .app_handle()
            .state::<DesktopHost>()
            .lock()?
            .windows
            .get_mut(window.label())
        {
            record.initial_bounds = Some(value);
        }
        return Ok(());
    }
    if platform::place_compositor_window(window, (value.x, value.y, value.width, value.height))? {
        return Ok(());
    }
    let displays = displays(window)?;
    let target = displays.iter().max_by(|left, right| {
        let overlap = |display: &Display| {
            let bounds = display.bounds;
            ((value.x + value.width).min(bounds.x + bounds.width) - value.x.max(bounds.x)).max(0.0)
                * ((value.y + value.height).min(bounds.y + bounds.height) - value.y.max(bounds.y))
                    .max(0.0)
        };
        overlap(left).total_cmp(&overlap(right))
    });
    let scale = target
        .map(|display| display.scale_factor)
        .unwrap_or(window.scale_factor().map_err(|error| error.to_string())?);
    window
        .set_position(tauri::PhysicalPosition::new(
            (value.x * scale).round() as i32,
            (value.y * scale).round() as i32,
        ))
        .map_err(|error| error.to_string())?;
    window
        .set_size(tauri::PhysicalSize::new(
            (value.width * scale).round() as u32,
            (value.height * scale).round() as u32,
        ))
        .map_err(|error| error.to_string())
}

fn initial_bounds(app: &tauri::AppHandle, role: WindowRole) -> Result<Bounds, String> {
    let main = app
        .get_webview_window("main")
        .ok_or("Pet window is unavailable")?;
    let work_areas: Vec<_> = displays(&main)?
        .into_iter()
        .map(|display| display.work_area)
        .collect();
    let work = work_areas.first().copied().unwrap_or(Bounds {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
    });
    let stored = {
        let host = app.state::<DesktopHost>();
        let state = host.lock()?;
        if role == WindowRole::ChatBall {
            state.layout.ball
        } else {
            state.layout.expanded.get(&role).copied()
        }
    };
    let (width, height) = match role {
        WindowRole::CompactChat => (480.0, 240.0),
        WindowRole::FullChat => (640.0, 640.0),
        WindowRole::ChatBall => (88.0, 88.0),
        WindowRole::Subtitle => (600.0, 100.0),
        WindowRole::SubtitleSettings => (370.0, 460.0),
        WindowRole::AgentHud => (360.0, 480.0),
        _ => (900.0, 700.0),
    };
    let default = Bounds {
        x: work.x + 24.0,
        y: work.y + work.height - height - 24.0,
        width,
        height,
    };
    Ok(stored
        .unwrap_or(default)
        .restore(&work_areas)
        .unwrap_or(default))
}

pub(crate) fn create(
    app: &tauri::AppHandle,
    role: WindowRole,
    label: String,
    url: WebviewUrl,
    parent: Option<String>,
    visible: bool,
) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(&label) {
        return Ok(window);
    }
    let geometry = initial_bounds(app, role)?;
    {
        let host = app.state::<DesktopHost>();
        let mut state = host.lock()?;
        if state.exiting {
            return Err("Desktop is exiting".into());
        }
        state.register(label.clone(), role, parent.clone(), visible);
        state.windows.get_mut(&label).unwrap().initial_bounds = Some(geometry);
    }
    let script = super::bridge::initialization_script(role, &label, parent.as_deref());
    let builder = WebviewWindowBuilder::new(app, &label, url)
        .title(format!("N.E.K.O [{}]", label))
        .initialization_script(script)
        .visible(false)
        .inner_size(geometry.width, geometry.height)
        .min_inner_size(1.0, 1.0)
        .position(geometry.x, geometry.y)
        .decorations(role.decorated())
        .transparent(!role.decorated())
        .shadow(role.decorated())
        .always_on_top(!role.decorated())
        .skip_taskbar(matches!(
            role,
            WindowRole::Subtitle | WindowRole::ChatBall | WindowRole::AgentHud
        ))
        .focusable(role.accepts_focus())
        .resizable(role.is_chat() || matches!(role, WindowRole::AgentHud | WindowRole::Child));
    let builder = if role.decorated() {
        if let Some(parent) = parent
            .as_deref()
            .and_then(|label| app.get_webview_window(label))
        {
            builder.parent(&parent).map_err(|error| error.to_string())?
        } else {
            builder
        }
    } else {
        builder
    };
    let result = builder.build().map_err(|error| error.to_string());
    let window = match result {
        Ok(window) => window,
        Err(error) => {
            app.state::<DesktopHost>().lock()?.windows.remove(&label);
            return Err(error);
        }
    };
    if let Err(error) = platform::configure_auxiliary_window(&window) {
        let _ = window.destroy();
        app.state::<DesktopHost>().lock()?.windows.remove(&label);
        return Err(error);
    }
    watch(&window);
    #[cfg(not(target_os = "linux"))]
    if role != WindowRole::Child {
        crate::radar::start(window.clone());
    }
    Ok(window)
}

pub(crate) fn watch(window: &WebviewWindow) {
    let label = window.label().to_owned();
    let app = window.app_handle().clone();
    window.on_window_event(move |event| {
        let host = app.state::<DesktopHost>();
        match event {
            WindowEvent::Moved(_)
            | WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {
                let Some(window) = app.get_webview_window(&label) else {
                    return;
                };
                let Ok(value) = bounds(&window) else {
                    return;
                };
                if let Ok(mut state) = host.lock() {
                    if let Some(record) = state.windows.get(&label) {
                        let role = record.role;
                        if record.ready && record.transient_bounds.is_none() {
                            if role == WindowRole::ChatBall {
                                state.layout.ball = Some(value);
                            } else if role != WindowRole::Pet && role != WindowRole::Child {
                                state.layout.expanded.insert(role, value);
                            }
                        }
                    }
                }
                schedule_save(&app);
                let _ = window.emit_to(window.label(), "neko-desktop-bounds-changed", value);
            }
            WindowEvent::CloseRequested { api, .. } => {
                let Ok(mut state) = host.lock() else {
                    return;
                };
                let exiting = state.exiting;
                let Some(record) = state.windows.get_mut(&label) else {
                    return;
                };
                if record.role == WindowRole::Pet && !exiting {
                    api.prevent_close();
                    state.exiting = true;
                    drop(state);
                    app.exit(0);
                    return;
                }
                if record.role != WindowRole::Pet && !record.role.decorated() && !exiting {
                    api.prevent_close();
                    record.desired_visible = false;
                    drop(state);
                    if let Some(window) = app.get_webview_window(&label) {
                        let _ = window.hide();
                    }
                }
            }
            WindowEvent::Destroyed => {
                let children: Vec<_> = host
                    .lock()
                    .map(|state| {
                        state
                            .windows
                            .iter()
                            .filter(|(_, record)| {
                                record.parent.as_deref() == Some(label.as_str())
                                    && record.role.decorated()
                            })
                            .map(|(label, _)| label.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for child in children {
                    if let Some(child) = app.get_webview_window(&child) {
                        let _ = child.destroy();
                    }
                }
                let parent = host
                    .lock()
                    .ok()
                    .and_then(|mut state| state.windows.remove(&label))
                    .and_then(|record| record.parent);
                if let Some(parent) = parent {
                    if let Some(window) = app.get_webview_window(&parent) {
                        let _ = window.emit_to(window.label(), "neko-desktop-child-closed", &label);
                    }
                }
            }
            _ => {}
        }
    });
}

pub(crate) fn apply_visibility(app: &tauri::AppHandle) -> Result<(), String> {
    let host = app.state::<DesktopHost>();
    let plan = host.lock()?.visibility_plan();
    let surface = plan.surface;
    let minimized = plan.minimized;
    for (label, visible) in plan.windows {
        if let Some(window) = app.get_webview_window(&label) {
            let was_visible = window.is_visible().unwrap_or(false);
            if visible {
                window.show()
            } else {
                window.hide()
            }
            .map_err(|error| error.to_string())?;
            if visible && !was_visible {
                let pending = host
                    .lock()?
                    .windows
                    .get(&label)
                    .and_then(|record| record.initial_bounds);
                if let Some(bounds) = pending {
                    platform::after_satellite_reveal(
                        &window,
                        (bounds.x, bounds.y, bounds.width, bounds.height),
                    );
                }
            }
            window
                .emit_to(
                    window.label(),
                    "neko-desktop-visibility",
                    serde_json::json!({ "visible":visible,
                "surface":surface,"minimized":minimized }),
                )
                .map_err(|error| error.to_string())?;
        }
    }
    if plan.restored {
        if let Some(ball) = app.get_webview_window(WindowRole::ChatBall.label()) {
            ball.emit_to(ball.label(), "neko-desktop-restore-ack", surface)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub(crate) fn schedule_save(app: &tauri::AppHandle) {
    let revision = app
        .state::<DesktopHost>()
        .save_revision
        .fetch_add(1, Ordering::SeqCst)
        + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let Some(host) = app.try_state::<DesktopHost>() else {
            return;
        };
        if host.save_revision.load(Ordering::SeqCst) == revision {
            if let Ok(state) = host.lock() {
                if let Err(error) = state.layout.save(&host.layout_path) {
                    eprintln!("Saving desktop layout failed: {error}");
                }
            }
        }
    });
}
