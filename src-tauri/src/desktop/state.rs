use super::{
    geometry::Bounds,
    layout::Layout,
    role::{ChatSurface, WindowRole},
    router::Router,
};
use std::collections::HashMap;

#[derive(Debug)]
pub(crate) struct WindowRecord {
    pub(crate) role: WindowRole,
    pub(crate) parent: Option<String>,
    pub(crate) ready: bool,
    pub(crate) painted: bool,
    pub(crate) desired_visible: bool,
    pub(crate) document: u64,
    pub(crate) started_at: f64,
    pub(crate) transient_bounds: Option<Bounds>,
    pub(crate) initial_bounds: Option<Bounds>,
}

pub(crate) struct DesktopState {
    pub(crate) windows: HashMap<String, WindowRecord>,
    pub(crate) layout: Layout,
    pub(crate) router: Router,
    pub(crate) base_url: Option<tauri::Url>,
    pub(crate) minimized: bool,
    pub(crate) restore_pending: Option<WindowRole>,
    pub(crate) exiting: bool,
    pub(crate) next_child: u64,
    pub(crate) menu_owner: Option<(String, u64)>,
    next_document: u64,
}

impl DesktopState {
    pub(crate) fn new(layout: Layout) -> Self {
        Self {
            windows: HashMap::new(),
            layout,
            router: Router::default(),
            base_url: None,
            minimized: false,
            restore_pending: None,
            exiting: false,
            next_child: 0,
            menu_owner: None,
            next_document: 0,
        }
    }

    pub(crate) fn register(
        &mut self,
        label: String,
        role: WindowRole,
        parent: Option<String>,
        visible: bool,
    ) {
        self.windows.insert(
            label,
            WindowRecord {
                role,
                parent,
                ready: false,
                painted: false,
                desired_visible: visible,
                document: 0,
                started_at: 0.0,
                transient_bounds: None,
                initial_bounds: None,
            },
        );
    }

    pub(crate) fn ready(&mut self, label: &str, started_at: f64) -> Result<u64, String> {
        if !started_at.is_finite() || started_at <= 0.0 {
            return Err("Invalid document identity".into());
        }
        let record = self
            .windows
            .get_mut(label)
            .ok_or("Unmanaged desktop window")?;
        if started_at < record.started_at {
            return Err("Obsolete document".into());
        }
        if started_at == record.started_at && record.ready {
            return Ok(record.document);
        }
        self.next_document += 1;
        record.document = self.next_document;
        record.started_at = started_at;
        record.ready = true;
        if record.role == WindowRole::Pet {
            self.router.pet_reloaded();
        }
        Ok(record.document)
    }

    pub(crate) fn sender(&self, label: &str, document: u64) -> Result<WindowRole, String> {
        let record = self.windows.get(label).ok_or("Unmanaged desktop window")?;
        if !record.ready || record.document != document || document == 0 {
            return Err("Obsolete document".into());
        }
        Ok(record.role)
    }

    pub(crate) fn select_surface(&mut self, surface: ChatSurface) {
        self.restore_pending = None;
        self.layout.chat_surface = surface;
        self.minimized = false;
        for record in self.windows.values_mut() {
            if record.role.is_chat() {
                record.desired_visible = record.role == surface.role();
            }
            if record.role == WindowRole::ChatBall {
                record.desired_visible = false;
            }
        }
    }

    pub(crate) fn minimize(&mut self) {
        self.restore_pending = None;
        self.minimized = true;
        for record in self.windows.values_mut() {
            if record.role.is_chat() {
                record.desired_visible = false;
            }
            if record.role == WindowRole::ChatBall {
                record.desired_visible = true;
            }
        }
    }

    pub(crate) fn restore(&mut self) {
        self.select_surface(self.layout.chat_surface);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_ready_cannot_reveal_the_chat_surface_user_has_switched_away_from() {
        let mut state = DesktopState::new(Layout::default());
        state.register("chat-compact".into(), WindowRole::CompactChat, None, true);
        state.register("chat-full".into(), WindowRole::FullChat, None, false);
        state.select_surface(ChatSurface::Full);
        state.ready("chat-compact", 10.0).unwrap();
        assert!(!state.windows["chat-compact"].desired_visible);
        assert!(state.windows["chat-full"].desired_visible);
    }

    #[test]
    fn minimized_chat_restores_the_last_selected_surface() {
        let mut state = DesktopState::new(Layout::default());
        state.register("chat-full".into(), WindowRole::FullChat, None, false);
        state.register("chat-ball".into(), WindowRole::ChatBall, None, false);
        state.select_surface(ChatSurface::Full);
        state.minimize();
        assert!(state.windows["chat-ball"].desired_visible);
        state.restore();
        assert!(state.windows["chat-full"].desired_visible);
        assert!(!state.windows["chat-ball"].desired_visible);
    }

    #[test]
    fn old_page_completion_cannot_replace_the_current_document() {
        let mut state = DesktopState::new(Layout::default());
        state.register("main".into(), WindowRole::Pet, None, true);
        let old = state.ready("main", 10.0).unwrap();
        let current = state.ready("main", 11.0).unwrap();
        assert!(state.sender("main", old).is_err());
        assert!(state.ready("main", 10.0).is_err());
        assert_eq!(state.sender("main", current).unwrap(), WindowRole::Pet);
    }
}
