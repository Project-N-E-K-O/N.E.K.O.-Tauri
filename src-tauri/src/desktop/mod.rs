//! Native multi-window host. Shared pages cross this seam through document-start
//! bridges; platform details and window/session ownership stay inside the host.

mod bridge;
pub(crate) mod commands;
mod feature;
mod geometry;
mod layout;
mod menu;
mod monitors;
mod role;
mod router;
mod state;
mod windows;

pub(crate) use commands::open_child;
pub(crate) use role::WindowRole;

use std::{
    path::PathBuf,
    sync::{atomic::AtomicU64, Mutex, MutexGuard},
};

pub(crate) struct DesktopHost {
    state: Mutex<state::DesktopState>,
    layout_path: PathBuf,
    save_revision: AtomicU64,
}

impl DesktopHost {
    pub(crate) fn new(layout_path: PathBuf) -> Self {
        let layout = layout::Layout::load(&layout_path);
        Self {
            state: Mutex::new(state::DesktopState::new(layout)),
            layout_path,
            save_revision: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, state::DesktopState>, String> {
        self.state
            .lock()
            .map_err(|_| "Desktop state is unavailable".into())
    }
}

pub(crate) fn create_main(app: &tauri::App) -> Result<tauri::WebviewWindow, String> {
    use tauri::Manager;
    app.manage(DesktopHost::new(
        app.path()
            .app_config_dir()
            .map_err(|error| error.to_string())?
            .join("windows.json"),
    ));
    app.state::<DesktopHost>()
        .lock()?
        .register("main".into(), WindowRole::Pet, None, true);
    let window =
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
            .title("N.E.K.O")
            .inner_size(1920.0, 1080.0)
            .visible(false)
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .accept_first_mouse(true)
            .initialization_script(bridge::initialization_script(WindowRole::Pet, "main", None))
            .build()
            .map_err(|error| error.to_string())?;
    windows::watch(&window);
    menu::install(app.handle());
    monitors::start(app.handle().clone());
    Ok(window)
}

pub(crate) fn backend_ready(window: &tauri::WebviewWindow, url: tauri::Url) -> Result<(), String> {
    use tauri::Manager;
    window.app_handle().state::<DesktopHost>().lock()?.base_url = Some(url.clone());
    window.navigate(url).map_err(|error| error.to_string())
}

pub(crate) fn page_loaded(window: &tauri::WebviewWindow, finished: bool) -> Result<(), String> {
    use tauri::Manager;
    let app = window.app_handle();
    if std::env::var_os("NEKO_TAURI_BRIDGE_DIAGNOSTIC").is_some() {
        println!(
            "[DesktopPage] label={} finished={} url={}",
            window.label(),
            finished,
            window.url().map_err(|error| error.to_string())?
        );
    }
    let Some(host) = app.try_state::<DesktopHost>() else {
        return Ok(());
    };
    {
        let mut state = host.lock()?;
        if let Some(record) = state.windows.get_mut(window.label()) {
            if record.role != WindowRole::ChatBall || !finished {
                record.painted = finished;
            }
            if !finished {
                record.ready = false;
                record.content_ready = false;
            }
        }
    }
    if finished
        && window.label() == "main"
        && crate::is_backend_page(&window.url().map_err(|error| error.to_string())?)
    {
        let surface = host.lock()?.layout.chat_surface;
        commands::ensure_role(app, surface.role())?;
        let mut state = host.lock()?;
        if state.minimized {
            state.minimize();
        } else {
            state.select_surface(surface);
        }
    }
    if window.label() != "main"
        || crate::is_backend_page(&window.url().map_err(|error| error.to_string())?)
    {
        windows::apply_visibility(app)?;
    }
    Ok(())
}

pub(crate) fn exiting(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(host) = app.try_state::<DesktopHost>() {
        if let Ok(mut state) = host.lock() {
            state.exiting = true;
            if let Err(error) = state.layout.save(&host.layout_path) {
                eprintln!("Saving desktop layout failed: {error}");
            }
        }
    }
}
