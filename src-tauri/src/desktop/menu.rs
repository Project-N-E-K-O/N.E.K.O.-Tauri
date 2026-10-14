use super::{
    commands::{self, WindowRequest},
    role::{ChatSurface, WindowRole},
    DesktopHost,
};
use tauri::{
    menu::{ContextMenu, MenuBuilder},
    Manager, WebviewWindow,
};

pub(crate) fn install(app: &tauri::AppHandle) {
    app.on_menu_event(|app, event| {
        let action = match event.id().as_ref() {
            "neko-window-compact" => WindowRequest::SelectSurface {
                surface: ChatSurface::Compact,
            },
            "neko-window-full" => WindowRequest::SelectSurface {
                surface: ChatSurface::Full,
            },
            "neko-window-restore" => WindowRequest::Expand { keep_ball: false },
            "neko-window-hud" => WindowRequest::Show {
                role: Some(WindowRole::AgentHud),
            },
            _ => return,
        };
        let owner = app
            .state::<DesktopHost>()
            .lock()
            .ok()
            .and_then(|state| state.menu_owner.clone());
        let Some((label, document)) = owner else {
            return;
        };
        let Some(window) = app.get_webview_window(&label) else {
            return;
        };
        let role = app
            .state::<DesktopHost>()
            .lock()
            .ok()
            .and_then(|state| state.sender(&label, document).ok());
        if let Some(role) = role {
            if let Err(error) = commands::dispatch(app, &window, role, action) {
                eprintln!("Desktop window menu action failed: {error}");
            }
        }
    });
}

pub(crate) fn popup(window: &WebviewWindow, document: u64) -> Result<(), String> {
    let app = window.app_handle();
    app.state::<DesktopHost>().lock()?.menu_owner = Some((window.label().into(), document));
    let menu = MenuBuilder::new(app)
        .text("neko-window-compact", "紧凑聊天")
        .text("neko-window-full", "完整聊天")
        .text("neko-window-restore", "恢复聊天")
        .separator()
        .text("neko-window-hud", "任务面板")
        .build()
        .map_err(|error| error.to_string())?;
    menu.popup(window.as_ref().window())
        .map_err(|error| error.to_string())
}
