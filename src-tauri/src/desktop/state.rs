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
    pub(crate) content_ready: bool,
    pub(crate) required_snapshot: u64,
    pub(crate) desired_visible: bool,
    pub(crate) document: u64,
    pub(crate) started_at: f64,
    pub(crate) activated_at: f64,
    pub(crate) transient_bounds: Option<Bounds>,
    pub(crate) initial_bounds: Option<Bounds>,
}

impl WindowRecord {
    fn can_show(&self) -> bool {
        self.ready && self.painted && (!self.role.is_chat() || self.content_ready)
    }
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
                content_ready: false,
                required_snapshot: 0,
                desired_visible: visible,
                document: 0,
                started_at: 0.0,
                activated_at: 0.0,
                transient_bounds: None,
                initial_bounds: None,
            },
        );
    }

    pub(crate) fn ready(
        &mut self,
        label: &str,
        started_at: f64,
        activated_at: f64,
    ) -> Result<u64, String> {
        if !started_at.is_finite()
            || started_at <= 0.0
            || !activated_at.is_finite()
            || activated_at < started_at
        {
            return Err("Invalid document identity".into());
        }
        let record = self
            .windows
            .get_mut(label)
            .ok_or("Unmanaged desktop window")?;
        // A cached document has an older time origin but a new activation.
        // Order activations, so its return is valid while queued IPC from the
        // previous activation cannot take over the current document.
        if activated_at < record.activated_at {
            return Err("Obsolete document".into());
        }
        if activated_at == record.activated_at && started_at == record.started_at && record.ready {
            return Ok(record.document);
        }
        self.next_document += 1;
        record.document = self.next_document;
        record.started_at = started_at;
        record.activated_at = activated_at;
        record.ready = true;
        record.content_ready = false;
        record.required_snapshot = 0;
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

    pub(crate) fn require_snapshot(&mut self, label: &str, sequence: u64) -> Result<(), String> {
        let record = self.windows.get_mut(label).ok_or("Window is unavailable")?;
        if sequence > record.required_snapshot {
            record.required_snapshot = sequence;
            record.content_ready = false;
        }
        Ok(())
    }

    pub(crate) fn content_ready(&mut self, label: &str, sequence: u64) -> Result<bool, String> {
        let record = self.windows.get_mut(label).ok_or("Window is unavailable")?;
        if sequence != record.required_snapshot {
            return Ok(false);
        }
        record.content_ready = true;
        Ok(true)
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

    pub(crate) fn begin_restore(&mut self, keep_ball: bool) {
        self.select_surface(self.layout.chat_surface);
        self.restore_pending = Some(self.layout.chat_surface.role());
        if let Some(target) = self
            .windows
            .get_mut(self.layout.chat_surface.role().label())
        {
            target.content_ready = false;
        }
        if keep_ball {
            if let Some(ball) = self.windows.get_mut(WindowRole::ChatBall.label()) {
                ball.desired_visible = true;
            }
        }
    }

    pub(crate) fn visibility_plan(&mut self) -> VisibilityPlan {
        let restored = self.restore_pending.filter(|target| {
            self.windows
                .get(target.label())
                .is_some_and(WindowRecord::can_show)
        });
        if restored.is_some() {
            self.restore_pending = None;
        }
        let ball_ready = self
            .windows
            .get(WindowRole::ChatBall.label())
            .is_some_and(WindowRecord::can_show);
        let windows = self
            .windows
            .iter()
            .map(|(label, record)| {
                let visible = record.can_show()
                    && (record.desired_visible
                        || (record.role == WindowRole::ChatBall && self.restore_pending.is_some())
                        || (self.minimized
                            && !ball_ready
                            && record.role == self.layout.chat_surface.role()));
                (label.clone(), visible)
            })
            .collect();
        VisibilityPlan {
            windows,
            restored: restored.is_some(),
            surface: self.layout.chat_surface,
            minimized: self.minimized,
        }
    }
}

pub(crate) struct VisibilityPlan {
    pub(crate) windows: Vec<(String, bool)>,
    pub(crate) restored: bool,
    pub(crate) surface: ChatSurface,
    pub(crate) minimized: bool,
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
        state.ready("chat-compact", 10.0, 10.0).unwrap();
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
        state.begin_restore(false);
        assert!(state.windows["chat-full"].desired_visible);
        assert!(!state.windows["chat-ball"].desired_visible);
    }

    #[test]
    fn old_page_completion_cannot_replace_the_current_document() {
        let mut state = DesktopState::new(Layout::default());
        state.register("main".into(), WindowRole::Pet, None, true);
        let old = state.ready("main", 10.0, 10.0).unwrap();
        let current = state.ready("main", 11.0, 11.0).unwrap();
        assert!(state.sender("main", old).is_err());
        assert!(state.ready("main", 10.0, 10.0).is_err());
        assert_eq!(state.sender("main", current).unwrap(), WindowRole::Pet);
    }

    #[test]
    fn cached_document_gets_a_new_identity_without_accepting_its_old_activation() {
        let mut state = DesktopState::new(Layout::default());
        state.register("child".into(), WindowRole::Child, None, true);
        let first = state.ready("child", 10.0, 10.1).unwrap();
        let second = state.ready("child", 11.0, 11.1).unwrap();
        let restored = state.ready("child", 10.0, 12.0).unwrap();
        assert!(state.sender("child", first).is_err());
        assert!(state.sender("child", second).is_err());
        assert!(state.ready("child", 11.0, 11.1).is_err());
        assert_eq!(state.sender("child", restored).unwrap(), WindowRole::Child);
    }

    #[test]
    fn chat_and_ball_keep_a_visible_restore_control_until_the_destination_is_ready() {
        let mut state = DesktopState::new(Layout::default());
        state.register("chat-compact".into(), WindowRole::CompactChat, None, true);
        state.register("chat-ball".into(), WindowRole::ChatBall, None, false);
        state.ready("chat-compact", 10.0, 10.0).unwrap();
        state.windows.get_mut("chat-compact").unwrap().painted = true;
        state.windows.get_mut("chat-compact").unwrap().content_ready = true;
        state.minimize();
        let visible = |plan: &VisibilityPlan, label: &str| {
            plan.windows
                .iter()
                .any(|(name, visible)| name == label && *visible)
        };
        assert!(visible(&state.visibility_plan(), "chat-compact"));
        state.ready("chat-ball", 11.0, 11.0).unwrap();
        state.windows.get_mut("chat-ball").unwrap().painted = true;
        let plan = state.visibility_plan();
        assert!(!visible(&plan, "chat-compact"));
        assert!(visible(&plan, "chat-ball"));
        state.windows.get_mut("chat-compact").unwrap().ready = false;
        state.begin_restore(false);
        let plan = state.visibility_plan();
        assert!(!plan.restored);
        assert!(visible(&plan, "chat-ball"));
        state.ready("chat-compact", 12.0, 12.0).unwrap();
        let plan = state.visibility_plan();
        assert!(!plan.restored);
        assert!(visible(&plan, "chat-ball"));
        state.windows.get_mut("chat-compact").unwrap().content_ready = true;
        let plan = state.visibility_plan();
        assert!(plan.restored);
        assert!(visible(&plan, "chat-compact"));
        assert!(!visible(&plan, "chat-ball"));
        assert!(!state.visibility_plan().restored);
    }

    #[test]
    fn a_mounted_sibling_waits_for_the_current_handoff_before_reveal() {
        let mut state = DesktopState::new(Layout::default());
        state.register("chat-full".into(), WindowRole::FullChat, None, false);
        state.ready("chat-full", 10.0, 10.0).unwrap();
        state.windows.get_mut("chat-full").unwrap().painted = true;
        state.content_ready("chat-full", 0).unwrap();
        state.require_snapshot("chat-full", 20).unwrap();
        state.select_surface(ChatSurface::Full);
        assert!(!state.visibility_plan().windows[0].1);
        state.require_snapshot("chat-full", 21).unwrap();
        assert!(!state.content_ready("chat-full", 20).unwrap());
        assert!(!state.visibility_plan().windows[0].1);
        state.content_ready("chat-full", 21).unwrap();
        assert!(state.visibility_plan().windows[0].1);
    }
}
