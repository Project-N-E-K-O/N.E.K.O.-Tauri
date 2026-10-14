use super::{geometry::Bounds, role::WindowRole, windows, DesktopHost};
use crate::platform;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{Emitter, Manager, WebviewUrl};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct FeatureOptions {
    pub(crate) name: Option<String>,
    pub(crate) navigate: bool,
    pub(crate) width: Option<f64>,
    pub(crate) height: Option<f64>,
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
}

pub(crate) fn open(
    app: &tauri::AppHandle,
    parent: &str,
    raw_url: &str,
    options: FeatureOptions,
) -> Result<Value, String> {
    let host = app.state::<DesktopHost>();
    let base = host
        .lock()?
        .base_url
        .clone()
        .ok_or("Backend is not ready")?;
    let url = base.join(raw_url).map_err(|error| error.to_string())?;
    if url.origin() != base.origin() {
        platform::open_external_url(&url)?;
        return Ok(Value::Null);
    }
    let label = {
        let mut state = host.lock()?;
        if state.exiting {
            return Err("Desktop is exiting".into());
        }
        match options
            .name
            .as_deref()
            .filter(|name| !name.is_empty() && *name != "_blank")
        {
            Some(name) => {
                use std::hash::{Hash, Hasher};
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                name.hash(&mut hash);
                format!("child-{:016x}", hash.finish())
            }
            None => {
                state.next_child += 1;
                format!("child-{}", state.next_child)
            }
        }
    };
    let existed = app.get_webview_window(&label).is_some();
    let window = windows::create(
        app,
        WindowRole::Child,
        label.clone(),
        WebviewUrl::External(url.clone()),
        Some(parent.into()),
        true,
    )?;
    if existed {
        let previous = host
            .lock()?
            .windows
            .get_mut(&label)
            .ok_or("Window is unavailable")?
            .parent
            .replace(parent.into());
        if previous.as_deref() != Some(parent) {
            if let Some(previous) = previous.and_then(|label| app.get_webview_window(&label)) {
                previous
                    .emit_to(previous.label(), "neko-desktop-child-closed", &label)
                    .map_err(|error| error.to_string())?;
            }
            window
                .emit_to(window.label(), "neko-desktop-parent-changed", parent)
                .map_err(|error| error.to_string())?;
        }
        if options.navigate {
            window.navigate(url).map_err(|error| error.to_string())?;
        }
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
    } else if options.width.is_some()
        || options.height.is_some()
        || options.x.is_some()
        || options.y.is_some()
    {
        let current = windows::bounds(&window)?;
        let requested = Bounds {
            x: options.x.unwrap_or(current.x),
            y: options.y.unwrap_or(current.y),
            width: options.width.unwrap_or(current.width),
            height: options.height.unwrap_or(current.height),
        };
        if !requested.valid() {
            let _ = window.destroy();
            return Err("Invalid feature window geometry".into());
        }
        let areas: Vec<_> = windows::displays(&window)?
            .into_iter()
            .map(|display| display.work_area)
            .collect();
        windows::set_bounds(&window, requested.restore(&areas).unwrap_or(requested))?;
    }
    Ok(json!({"label":label}))
}
