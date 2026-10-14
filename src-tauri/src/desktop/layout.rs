use super::{
    geometry::Bounds,
    role::{ChatSurface, WindowRole},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Layout {
    pub(crate) chat_surface: ChatSurface,
    pub(crate) expanded: HashMap<WindowRole, Bounds>,
    pub(crate) ball: Option<Bounds>,
}

impl Layout {
    pub(crate) fn load(path: &Path) -> Self {
        let mut layout: Self = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        layout.expanded.retain(|_, bounds| bounds.valid());
        layout.ball = layout.ball.filter(|bounds| bounds.valid());
        layout
    }

    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let temporary = temporary_path(path);
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("{}.tmp", std::process::id()))
}
