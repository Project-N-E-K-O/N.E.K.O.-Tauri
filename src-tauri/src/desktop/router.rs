use super::role::WindowRole;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

const MAX_REPLAY_FRAMES: usize = 256;
const MAX_REPLAY_BYTES: usize = 2 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum MessageKind {
    SessionConnecting,
    SessionReady,
    SessionClosed,
    SocketMessage,
    SocketSend,
    ConfigRequest,
    ConfigResult,
    AvatarRequest,
    AvatarResult,
    CharacterReferenceRequest,
    CharacterReferenceResult,
    ScreenshotRequest,
    ScreenshotResult,
    Transcript,
    SubtitleState,
    SubtitleSettingsState,
    SubtitleChange,
    AgentUpdate,
    AgentToggle,
    SettingsRequest,
    SettingsState,
    SettingsChange,
    ChatSnapshot,
    ChatRestore,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Message {
    pub(crate) kind: MessageKind,
    #[serde(default)]
    pub(crate) data: Value,
    pub(crate) document: u64,
    #[serde(default)]
    pub(crate) generation: u64,
    #[serde(default)]
    pub(crate) epoch: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivery {
    pub(crate) kind: MessageKind,
    pub(crate) data: Value,
    pub(crate) epoch: u64,
    pub(crate) generation: u64,
    pub(crate) sequence: u64,
}

#[derive(Default)]
pub(crate) struct Router {
    epoch: u64,
    generation: u64,
    sequence: u64,
    session: Option<Delivery>,
    sticky: HashMap<MessageKind, Delivery>,
    frames: VecDeque<Delivery>,
    replay_bytes: usize,
}

impl MessageKind {
    fn allowed_sender(self, role: WindowRole) -> bool {
        use MessageKind::*;
        match self {
            SessionConnecting
            | SessionReady
            | SessionClosed
            | SocketMessage
            | ConfigResult
            | AvatarResult
            | CharacterReferenceResult
            | ScreenshotResult
            | Transcript
            | SubtitleState
            | AgentUpdate
            | SettingsState => role == WindowRole::Pet,
            SocketSend
            | ConfigRequest
            | AvatarRequest
            | CharacterReferenceRequest
            | ScreenshotRequest
            | ChatSnapshot => role.is_chat(),
            SubtitleChange => {
                matches!(role, WindowRole::Subtitle | WindowRole::SubtitleSettings)
                    || role.is_chat()
            }
            SubtitleSettingsState => role == WindowRole::Subtitle,
            AgentToggle => role == WindowRole::AgentHud,
            SettingsRequest => role != WindowRole::Pet && role != WindowRole::ChatBall,
            SettingsChange => role.is_chat() || role == WindowRole::Child,
            ChatRestore => false,
        }
    }

    pub(crate) fn targets(self) -> &'static [WindowRole] {
        use MessageKind::*;
        use WindowRole::*;
        match self {
            SessionConnecting
            | SessionReady
            | SessionClosed
            | SocketMessage
            | ConfigResult
            | AvatarResult
            | CharacterReferenceResult
            | ScreenshotResult
            | ChatRestore => &[CompactChat, FullChat],
            SocketSend
            | ConfigRequest
            | AvatarRequest
            | CharacterReferenceRequest
            | ScreenshotRequest
            | AgentToggle
            | SettingsRequest => &[Pet],
            SubtitleChange | SettingsChange => &[Pet, CompactChat, FullChat],
            Transcript => &[Subtitle],
            SubtitleState => &[Subtitle, SubtitleSettings, CompactChat, FullChat],
            SubtitleSettingsState => &[SubtitleSettings],
            AgentUpdate => &[AgentHud],
            SettingsState => &[CompactChat, FullChat, Child, Subtitle, SubtitleSettings, AgentHud],
            ChatSnapshot => &[CompactChat, FullChat],
        }
    }

    fn sticky(self) -> bool {
        matches!(
            self,
            Self::ConfigResult
                | Self::AvatarResult
                | Self::Transcript
                | Self::SubtitleState
                | Self::SubtitleSettingsState
                | Self::SettingsState
                | Self::ChatSnapshot
                | Self::AgentUpdate
        )
    }
}

impl Router {
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
    pub(crate) fn pet_reloaded(&mut self) {
        self.epoch += 1;
        self.generation = 0;
        self.session = None;
        self.frames.clear();
        self.replay_bytes = 0;
        self.sticky.clear();
    }

    /// Sender identity and document freshness are checked by the host before
    /// this seam. Routing still enforces role authority and socket generation.
    pub(crate) fn publish(
        &mut self,
        role: WindowRole,
        message: Message,
    ) -> Result<Delivery, String> {
        if !message.kind.allowed_sender(role) {
            return Err("This window cannot publish that message".into());
        }
        if message.kind == MessageKind::SocketSend && message.epoch != self.epoch {
            return Err("Obsolete Pet document epoch".into());
        }
        let size = serde_json::to_vec(&message.data)
            .map_err(|error| error.to_string())?
            .len();
        if size > MAX_MESSAGE_BYTES {
            return Err("Desktop message exceeds the payload limit".into());
        }
        use MessageKind::*;
        if message.kind == SessionConnecting {
            if message.generation <= self.generation {
                return Err("Obsolete socket generation".into());
            }
            self.generation = message.generation;
            self.frames.clear();
            self.replay_bytes = 0;
        } else if matches!(
            message.kind,
            SessionReady | SessionClosed | SocketMessage | SocketSend
        ) && (message.generation == 0 || message.generation != self.generation)
        {
            return Err("Obsolete socket generation".into());
        }
        if matches!(message.kind, SocketMessage | SocketSend)
            && self.session.as_ref().map(|state| state.kind) != Some(SessionReady)
        {
            return Err("Character socket is not open".into());
        }
        self.sequence += 1;
        let delivery = Delivery {
            kind: message.kind,
            data: message.data,
            epoch: self.epoch,
            generation: self.generation,
            sequence: self.sequence,
        };
        if matches!(
            message.kind,
            SessionConnecting | SessionReady | SessionClosed
        ) {
            self.session = Some(delivery.clone());
        }
        if message.kind.sticky() {
            self.sticky.insert(message.kind, delivery.clone());
        }
        if message.kind == SocketMessage {
            self.frames.push_back(delivery.clone());
            self.replay_bytes += size;
            while self.frames.len() > MAX_REPLAY_FRAMES || self.replay_bytes > MAX_REPLAY_BYTES {
                if let Some(frame) = self.frames.pop_front() {
                    self.replay_bytes = self.replay_bytes.saturating_sub(
                        serde_json::to_vec(&frame.data)
                            .map(|bytes| bytes.len())
                            .unwrap_or(0),
                    );
                }
            }
        }
        Ok(delivery)
    }

    pub(crate) fn replay(&self, role: WindowRole) -> Vec<Delivery> {
        let mut replay = Vec::new();
        let snapshot = self.sticky.get(&MessageKind::ChatSnapshot);
        let through_sequence = snapshot
            .and_then(|message| message.data.get("throughSequence"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if role.is_chat() {
            if let Some(snapshot) = snapshot {
                replay.push(snapshot.clone());
            }
        }
        if role.is_chat() {
            if let Some(session) = &self.session {
                replay.push(session.clone());
                if session.kind == MessageKind::SessionReady {
                    replay.extend(
                        self.frames
                            .iter()
                            .filter(|frame| frame.sequence > through_sequence)
                            .cloned(),
                    );
                }
            }
        }
        let mut sticky: Vec<_> = self
            .sticky
            .values()
            .filter(|message| {
                message.kind.targets().contains(&role)
                    && (!role.is_chat() || message.kind != MessageKind::ChatSnapshot)
            })
            .cloned()
            .collect();
        sticky.sort_by_key(|message| message.sequence);
        replay.extend(sticky);
        replay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(kind: MessageKind, generation: u64, data: Value) -> Message {
        Message {
            kind,
            document: 1,
            generation,
            epoch: 0,
            data,
        }
    }

    #[test]
    fn late_chat_receives_open_before_first_response() {
        let mut router = Router::default();
        router.pet_reloaded();
        for (kind, data) in [
            (
                MessageKind::SessionConnecting,
                json!({"url":"ws://127.0.0.1:48911/ws/cat"}),
            ),
            (MessageKind::SessionReady, json!({})),
            (
                MessageKind::SocketMessage,
                json!("{\"type\":\"gemini_response\",\"text\":\"hello\"}"),
            ),
        ] {
            router
                .publish(WindowRole::Pet, message(kind, 1, data))
                .unwrap();
        }
        let replay = router.replay(WindowRole::FullChat);
        assert_eq!(
            replay.iter().map(|item| item.kind).collect::<Vec<_>>(),
            vec![MessageKind::SessionReady, MessageKind::SocketMessage]
        );
    }

    #[test]
    fn replaced_socket_cannot_close_or_write_to_current_chat() {
        let mut router = Router::default();
        router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SessionConnecting, 1, json!({})),
            )
            .unwrap();
        router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SessionConnecting, 2, json!({})),
            )
            .unwrap();
        assert!(router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SessionClosed, 1, json!({}))
            )
            .is_err());
        assert!(router
            .publish(
                WindowRole::CompactChat,
                message(MessageKind::SocketSend, 1, json!("old input"))
            )
            .is_err());
        assert_eq!(router.replay(WindowRole::CompactChat)[0].generation, 2);
    }

    #[test]
    fn child_cannot_claim_pet_session_authority() {
        let mut router = Router::default();
        assert!(router
            .publish(
                WindowRole::Child,
                message(MessageKind::SessionReady, 1, json!({}))
            )
            .is_err());
    }

    #[test]
    fn queued_input_from_previous_pet_document_cannot_reach_new_character() {
        let mut router = Router::default();
        router.pet_reloaded();
        let old_epoch = router.epoch();
        router.pet_reloaded();
        router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SessionConnecting, 1, json!({})),
            )
            .unwrap();
        router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SessionReady, 1, json!({})),
            )
            .unwrap();
        let mut queued = message(MessageKind::SocketSend, 1, json!("old input"));
        queued.epoch = old_epoch;
        assert!(router.publish(WindowRole::CompactChat, queued).is_err());
    }

    #[test]
    fn late_mirror_replays_only_frames_after_the_conversation_snapshot() {
        let mut router = Router::default();
        router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SessionConnecting, 1, json!({})),
            )
            .unwrap();
        router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SessionReady, 1, json!({})),
            )
            .unwrap();
        let old = router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SocketMessage, 1, json!("older")),
            )
            .unwrap();
        router
            .publish(
                WindowRole::CompactChat,
                message(
                    MessageKind::ChatSnapshot,
                    1,
                    json!({"throughSequence":old.sequence,"messages":[]}),
                ),
            )
            .unwrap();
        router
            .publish(
                WindowRole::Pet,
                message(MessageKind::SocketMessage, 1, json!("newer")),
            )
            .unwrap();
        let replay = router.replay(WindowRole::FullChat);
        assert_eq!(replay[0].kind, MessageKind::ChatSnapshot);
        assert_eq!(
            replay
                .iter()
                .filter(|frame| frame.kind == MessageKind::SocketMessage)
                .map(|frame| frame.data.clone())
                .collect::<Vec<_>>(),
            vec![json!("newer")]
        );
    }
}
