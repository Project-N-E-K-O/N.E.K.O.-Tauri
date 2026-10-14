use super::{
    geometry::Bounds,
    role::{ChatSurface, WindowRole},
    router::Message,
    windows, DesktopHost,
};
use crate::platform;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow};

#[derive(Debug, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum WindowRequest {
    Bounds,
    WorkArea,
    Displays,
    Cursor,
    Painted,
    ContentReady {
        through_snapshot: u64,
    },
    WindowMenu,
    SubtitleSettings {
        visible: bool,
    },
    InputRegion {
        ignore: bool,
        rectangles: Vec<Bounds>,
        dpr: f64,
    },
    Show {
        role: Option<WindowRole>,
    },
    Hide {
        role: Option<WindowRole>,
    },
    Close,
    Focus,
    FocusParent,
    FocusChild {
        label: String,
    },
    NativeMinimize,
    ToggleMaximize,
    IsMaximized,
    PinState,
    SetPinned {
        pinned: bool,
    },
    SetBounds {
        bounds: Bounds,
        #[serde(default)]
        transient: bool,
    },
    RestoreBounds,
    SetResizable {
        resizable: bool,
    },
    Drag,
    Resize {
        direction: String,
    },
    SelectSurface {
        surface: ChatSurface,
    },
    Collapse {
        bounds: Option<Bounds>,
    },
    Expand {
        #[serde(default)]
        keep_ball: bool,
    },
    OpenChild {
        url: String,
        name: Option<String>,
        #[serde(default)]
        navigate: bool,
        #[serde(default)]
        width: Option<f64>,
        #[serde(default)]
        height: Option<f64>,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
    },
    CloseChild {
        label: String,
    },
    PostMessage {
        target: String,
        data: Value,
    },
}

fn valid_sender(window: &WebviewWindow, document: u64) -> Result<WindowRole, String> {
    // Native getters may marshal onto the UI thread. Never hold desktop state
    // while waiting for that thread, whose window callbacks also need state.
    let url = window.url().map_err(|error| error.to_string())?;
    let host = window.app_handle().state::<DesktopHost>();
    let state = host.lock()?;
    let role = state.sender(window.label(), document)?;
    if role == WindowRole::ChatBall && url.path().ends_with("/ball.html") {
        return Ok(role);
    }
    let base = state.base_url.as_ref().ok_or("Backend is not ready")?;
    if url.origin() != base.origin() {
        return Err("Page is outside the desktop backend origin".into());
    }
    Ok(role)
}

pub(crate) async fn on_main<T: Send + 'static>(
    app: tauri::AppHandle,
    operation: impl FnOnce(&tauri::AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let handle = app.clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    app.run_on_main_thread(move || {
        if std::time::Instant::now() >= deadline {
            let _ = sender.send(Err("Desktop operation expired before execution".into()));
            return;
        }
        let _ = sender.send(operation(&handle));
    })
    .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(std::time::Duration::from_secs(15))
            .map_err(|error| format!("Desktop operation did not finish: {error}"))
    })
    .await
    .map_err(|error| error.to_string())??
}

#[tauri::command]
pub(crate) async fn desktop_ready(
    window: WebviewWindow,
    started_at: f64,
    activated_at: f64,
) -> Result<Value, String> {
    let label = window.label().to_owned();
    on_main(window.app_handle().clone(), move |app| {
        let window = app.get_webview_window(&label).ok_or("Window was closed")?;
        let host = app.state::<DesktopHost>();
        let (document, role, parent, replay, surface, minimized, visible, backend_url, epoch) = {
            let mut state = host.lock()?;
            let record = state
                .windows
                .get(&label)
                .ok_or("Unmanaged desktop window")?;
            let role = record.role;
            let url = window.url().map_err(|error| error.to_string())?;
            if role != WindowRole::ChatBall
                && state.base_url.as_ref().map(|base| base.origin()) != Some(url.origin())
            {
                return Err("Page is outside the desktop backend origin".into());
            }
            let document = state.ready(&label, started_at, activated_at)?;
            if role.is_chat() {
                let sequence = state
                    .router
                    .replay(role)
                    .iter()
                    .find(|delivery| delivery.kind == super::router::MessageKind::ChatSnapshot)
                    .map(|delivery| delivery.sequence)
                    .unwrap_or(0);
                state.require_snapshot(&label, sequence)?;
            }
            (
                document,
                role,
                state.windows[&label].parent.clone(),
                state.router.replay(role),
                state.layout.chat_surface,
                state.minimized,
                state.windows[&label].desired_visible,
                state.base_url.clone(),
                state.router.epoch(),
            )
        };
        windows::apply_visibility(app)?;
        Ok(
            json!({ "document": document, "role": role, "parent": parent, "replay": replay,
            "bounds": windows::bounds(&window)?, "surface": surface, "minimized": minimized,
            "visible": visible, "backendUrl": backend_url, "epoch": epoch }),
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn desktop_publish(window: WebviewWindow, message: Message) -> Result<(), String> {
    let role = valid_sender(&window, message.document)?;
    let app = window.app_handle();
    let host = app.state::<DesktopHost>();
    let delivery = {
        let mut state = host.lock()?;
        if state.sender(window.label(), message.document)? != role {
            return Err("Window identity changed".into());
        }
        if message.kind == super::router::MessageKind::ChatSnapshot
            && (state.layout.chat_surface.role() != role
                || state.minimized
                || !state
                    .windows
                    .get(window.label())
                    .is_some_and(|record| record.desired_visible))
        {
            return Err("Only the selected chat surface publishes conversation state".into());
        }
        state.router.publish(role, message)?
    };
    let receivers: Vec<_> = {
        let host = app.state::<DesktopHost>();
        let mut state = host.lock()?;
        let labels: Vec<_> = state
            .windows
            .iter()
            .filter(|(label, record)| {
                label.as_str() != window.label()
                    && record.ready
                    && delivery.kind.targets().contains(&record.role)
            })
            .map(|(label, _)| label.clone())
            .collect();
        if delivery.kind == super::router::MessageKind::ChatSnapshot {
            for label in &labels {
                state.require_snapshot(label, delivery.sequence)?;
            }
        }
        labels
    };
    for label in receivers {
        if let Some(receiver) = app.get_webview_window(&label) {
            receiver
                .emit_to(receiver.label(), "neko-desktop-message", &delivery)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn desktop_window(
    window: WebviewWindow,
    request: WindowRequest,
    document: u64,
) -> Result<Value, String> {
    let role = valid_sender(&window, document)?;
    let label = window.label().to_owned();
    on_main(window.app_handle().clone(), move |app| {
        let window = app.get_webview_window(&label).ok_or("Window was closed")?;
        // Recheck after the main-thread scheduling seam: reload may have
        // invalidated the sender while this operation was queued.
        if valid_sender(&window, document)? != role {
            return Err("Window identity changed".into());
        }
        dispatch(app, &window, role, request)
    })
    .await
}

fn controlled_role(
    sender: WindowRole,
    requested: Option<WindowRole>,
) -> Result<WindowRole, String> {
    let target = requested.unwrap_or(sender);
    if target == sender
        || (sender == WindowRole::Pet || sender.is_chat())
            && matches!(
                target,
                WindowRole::CompactChat
                    | WindowRole::FullChat
                    | WindowRole::Subtitle
                    | WindowRole::AgentHud
            )
    {
        Ok(target)
    } else {
        Err("This window cannot control that surface".into())
    }
}

pub(super) fn dispatch(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
    role: WindowRole,
    request: WindowRequest,
) -> Result<Value, String> {
    let host = app.state::<DesktopHost>();
    match request {
        WindowRequest::Bounds => Ok(json!(windows::bounds(window)?)),
        WindowRequest::Displays => Ok(json!(windows::displays(window)?)),
        WindowRequest::WorkArea => {
            let bounds = windows::bounds(window)?;
            let areas: Vec<_> = windows::displays(window)?
                .into_iter()
                .map(|display| display.work_area)
                .collect();
            let fitted = bounds.restore(&areas).ok_or("No monitor is available")?;
            let area = areas
                .into_iter()
                .find(|area| {
                    fitted.x >= area.x
                        && fitted.x < area.x + area.width
                        && fitted.y >= area.y
                        && fitted.y < area.y + area.height
                })
                .ok_or("No monitor is available")?;
            Ok(json!(area))
        }
        WindowRequest::Cursor => {
            let position = window
                .cursor_position()
                .map_err(|error| error.to_string())?;
            let scale = window.scale_factor().map_err(|error| error.to_string())?;
            Ok(json!({ "x": position.x / scale, "y": position.y / scale }))
        }
        WindowRequest::Painted => {
            host.lock()?
                .windows
                .get_mut(window.label())
                .ok_or("Window is unavailable")?
                .painted = true;
            windows::apply_visibility(app)?;
            Ok(Value::Null)
        }
        WindowRequest::ContentReady { through_snapshot } => {
            if !role.is_chat() {
                return Err("Only chat surfaces report content readiness".into());
            }
            let accepted = host
                .lock()?
                .content_ready(window.label(), through_snapshot)?;
            if accepted {
                windows::apply_visibility(app)?;
            }
            Ok(json!({"accepted":accepted}))
        }
        WindowRequest::WindowMenu => {
            if role != WindowRole::Pet && !role.is_chat() {
                return Err("This surface has no chat window menu".into());
            }
            let document = host.lock()?.windows[window.label()].document;
            super::menu::popup(window, document)?;
            Ok(Value::Null)
        }
        WindowRequest::SubtitleSettings { visible } => {
            if !matches!(role, WindowRole::Subtitle | WindowRole::SubtitleSettings) {
                return Err("Only the subtitle surface controls subtitle settings".into());
            }
            let settings = ensure_role(app, WindowRole::SubtitleSettings)?;
            host.lock()?
                .windows
                .get_mut(settings.label())
                .ok_or("Window is unavailable")?
                .desired_visible = visible;
            windows::apply_visibility(app)?;
            if visible {
                settings.set_focus().map_err(|error| error.to_string())?;
            }
            Ok(json!(true))
        }
        WindowRequest::InputRegion {
            ignore,
            rectangles,
            dpr,
        } => {
            if role == WindowRole::Pet || role == WindowRole::Child {
                return Err("Surface does not use satellite input regions".into());
            }
            if rectangles.len() > 128
                || !dpr.is_finite()
                || !(0.5..=8.0).contains(&dpr)
                || rectangles.iter().any(|rectangle| !rectangle.valid())
            {
                return Err("Invalid surface input region".into());
            }
            let details = json!({ "dpr":dpr, "persistentRects": rectangles.into_iter()
                .map(|rect| [rect.x, rect.y, rect.width, rect.height]).collect::<Vec<_>>() });
            platform::set_auxiliary_click_through(
                &window.as_ref().window(),
                ignore,
                &details.to_string(),
            )?;
            Ok(Value::Null)
        }
        WindowRequest::Show { role: requested } => {
            let target = controlled_role(role, requested)?;
            let label = if target == role {
                window.label()
            } else {
                target.label()
            };
            if target.is_chat()
                && role.is_chat()
                && host.lock()?.layout.chat_surface.role() != target
            {
                return Ok(json!(false));
            }
            if !matches!(target, WindowRole::Pet | WindowRole::Child) {
                ensure_role(app, target)?;
            }
            host.lock()?
                .windows
                .get_mut(label)
                .ok_or("Window is unavailable")?
                .desired_visible = true;
            windows::apply_visibility(app)?;
            Ok(json!(true))
        }
        WindowRequest::Hide { role: requested } => {
            let target = controlled_role(role, requested)?;
            let label = if target == role {
                window.label()
            } else {
                target.label()
            };
            if let Some(record) = host.lock()?.windows.get_mut(label) {
                record.desired_visible = false;
            }
            windows::apply_visibility(app)?;
            Ok(Value::Null)
        }
        WindowRequest::Close => {
            window.close().map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        WindowRequest::Focus => {
            window.set_focus().map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        WindowRequest::FocusParent => {
            let parent = host
                .lock()?
                .windows
                .get(window.label())
                .and_then(|record| record.parent.clone())
                .ok_or("Window has no parent")?;
            if let Some(parent) = app.get_webview_window(&parent) {
                parent.set_focus().map_err(|error| error.to_string())?;
            }
            Ok(Value::Null)
        }
        WindowRequest::FocusChild { label } => {
            if host
                .lock()?
                .windows
                .get(&label)
                .and_then(|record| record.parent.as_deref())
                != Some(window.label())
            {
                return Err("Window is not owned by this page".into());
            }
            if let Some(child) = app.get_webview_window(&label) {
                child.show().map_err(|error| error.to_string())?;
                child.set_focus().map_err(|error| error.to_string())?;
            }
            Ok(Value::Null)
        }
        WindowRequest::NativeMinimize => {
            window.minimize().map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        WindowRequest::ToggleMaximize => {
            if window.is_maximized().map_err(|error| error.to_string())? {
                window.unmaximize()
            } else {
                window.maximize()
            }
            .map_err(|error| error.to_string())?;
            Ok(json!(window
                .is_maximized()
                .map_err(|error| error.to_string())?))
        }
        WindowRequest::IsMaximized => Ok(json!(window
            .is_maximized()
            .map_err(|error| error.to_string())?)),
        WindowRequest::PinState => Ok(
            json!({"available":true,"pinned":window.is_always_on_top().map_err(|error| error.to_string())?}),
        ),
        WindowRequest::SetPinned { pinned } => {
            window
                .set_always_on_top(pinned)
                .map_err(|error| error.to_string())?;
            Ok(json!({"available":true,"pinned":pinned}))
        }
        WindowRequest::SetBounds { bounds, transient } => {
            if transient {
                let current = windows::bounds(window)?;
                let mut state = host.lock()?;
                let record = state
                    .windows
                    .get_mut(window.label())
                    .ok_or("Window is unavailable")?;
                record.transient_bounds.get_or_insert(current);
            }
            windows::set_bounds(window, bounds)?;
            Ok(json!(windows::bounds(window)?))
        }
        WindowRequest::RestoreBounds => {
            let saved = host
                .lock()?
                .windows
                .get_mut(window.label())
                .ok_or("Window is unavailable")?
                .transient_bounds
                .take();
            if let Some(bounds) = saved {
                windows::set_bounds(window, bounds)?;
            }
            Ok(json!(windows::bounds(window)?))
        }
        WindowRequest::SetResizable { resizable } => {
            window
                .set_resizable(resizable)
                .map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        WindowRequest::Drag => {
            window.start_dragging().map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        WindowRequest::Resize { direction } => {
            let direction = match direction.as_str() {
                "n" => "North",
                "s" => "South",
                "e" => "East",
                "w" => "West",
                "ne" => "NorthEast",
                "nw" => "NorthWest",
                "sw" => "SouthWest",
                "se" => "SouthEast",
                _ => return Err("Invalid resize direction".into()),
            };
            window
                .as_ref()
                .window()
                .start_resize_dragging(
                    serde_json::from_value(json!(direction)).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
        WindowRequest::SelectSurface { surface } => {
            if role != WindowRole::Pet && !role.is_chat() {
                return Err("This window cannot select chat surface".into());
            }
            ensure_role(app, surface.role())?;
            host.lock()?.select_surface(surface);
            windows::apply_visibility(app)?;
            windows::schedule_save(app);
            Ok(json!(surface))
        }
        WindowRequest::Collapse { bounds } => {
            if !role.is_chat() && role != WindowRole::Pet {
                return Err("Only chat can minimize".into());
            }
            let selected = host.lock()?.layout.chat_surface.role();
            let chat = ensure_role(app, selected)?;
            let current = windows::bounds(&chat)?;
            let target = bounds.unwrap_or(Bounds {
                x: current.x,
                y: current.y + current.height - 88.0,
                width: 88.0,
                height: 88.0,
            });
            if !target.valid() {
                return Err("Invalid ball bounds".into());
            }
            let ball = ensure_role(app, WindowRole::ChatBall)?;
            windows::set_bounds(&ball, target)?;
            {
                let mut state = host.lock()?;
                state.layout.expanded.insert(selected, current);
                state.layout.ball = Some(target);
                state.minimize();
            }
            windows::apply_visibility(app)?;
            windows::schedule_save(app);
            Ok(json!(target))
        }
        WindowRequest::Expand { keep_ball } => {
            if !role.is_chat() && role != WindowRole::ChatBall && role != WindowRole::Pet {
                return Err("This window cannot restore chat".into());
            }
            let surface = host.lock()?.layout.chat_surface;
            let chat = ensure_role(app, surface.role())?;
            let mut geometry = host
                .lock()?
                .layout
                .expanded
                .get(&surface.role())
                .copied()
                .unwrap_or(windows::bounds(&chat)?);
            let minimized = host.lock()?.minimized;
            if let Some(ball) = app
                .get_webview_window(WindowRole::ChatBall.label())
                .filter(|_| minimized)
            {
                let anchor = windows::bounds(&ball)?;
                geometry.x = anchor.x;
                geometry.y = anchor.y + anchor.height - geometry.height;
                let areas: Vec<_> = windows::displays(&chat)?
                    .into_iter()
                    .map(|display| display.work_area)
                    .collect();
                geometry = geometry.restore(&areas).unwrap_or(geometry);
                windows::set_bounds(&chat, geometry)?;
            }
            {
                let mut state = host.lock()?;
                state.begin_restore(keep_ball);
            }
            windows::apply_visibility(app)?;
            chat.emit_to(
                chat.label(),
                "neko-desktop-chat-restored",
                json!({"surface":surface,"bounds":geometry}),
            )
            .map_err(|error| error.to_string())?;
            windows::schedule_save(app);
            Ok(json!(geometry))
        }
        WindowRequest::OpenChild {
            url,
            name,
            navigate,
            width,
            height,
            x,
            y,
        } => super::feature::open(
            app,
            window.label(),
            &url,
            super::feature::FeatureOptions {
                name,
                navigate,
                width,
                height,
                x,
                y,
            },
        ),
        WindowRequest::CloseChild { label } => {
            if host
                .lock()?
                .windows
                .get(&label)
                .and_then(|record| record.parent.as_deref())
                != Some(window.label())
            {
                return Err("Window is not owned by this page".into());
            }
            if let Some(child) = app.get_webview_window(&label) {
                child.close().map_err(|error| error.to_string())?;
            }
            Ok(Value::Null)
        }
        WindowRequest::PostMessage { target, data } => {
            let state = host.lock()?;
            let own = state
                .windows
                .get(window.label())
                .ok_or("Window is unavailable")?;
            let other = state.windows.get(&target).ok_or("Window is unavailable")?;
            if own.parent.as_deref() != Some(target.as_str())
                && other.parent.as_deref() != Some(window.label())
            {
                return Err("Windows do not share a parent-child relationship".into());
            }
            drop(state);
            let receiver = app.get_webview_window(&target).ok_or("Window was closed")?;
            receiver
                .emit_to(
                    receiver.label(),
                    "neko-desktop-post-message",
                    json!({"source":window.label(),"data":data}),
                )
                .map_err(|error| error.to_string())?;
            Ok(Value::Null)
        }
    }
}

pub(crate) fn ensure_role(
    app: &tauri::AppHandle,
    role: WindowRole,
) -> Result<WebviewWindow, String> {
    if matches!(role, WindowRole::Pet | WindowRole::Child) {
        return Err("Role cannot be created as a satellite".into());
    }
    let base = app
        .state::<DesktopHost>()
        .lock()?
        .base_url
        .clone()
        .ok_or("Backend is not ready")?;
    let url = if role == WindowRole::ChatBall {
        WebviewUrl::App("ball.html".into())
    } else {
        WebviewUrl::External(
            base.join(role.route().ok_or("Role has no page")?)
                .map_err(|error| error.to_string())?,
        )
    };
    windows::create(
        app,
        role,
        role.label().into(),
        url,
        Some(
            if role == WindowRole::SubtitleSettings {
                "subtitle"
            } else {
                "main"
            }
            .into(),
        ),
        false,
    )
}

pub(crate) fn open_child(
    app: &tauri::AppHandle,
    parent: &str,
    raw_url: &str,
    name: Option<&str>,
) -> Result<Value, String> {
    super::feature::open(
        app,
        parent,
        raw_url,
        super::feature::FeatureOptions {
            name: name.map(str::to_owned),
            ..Default::default()
        },
    )
}
