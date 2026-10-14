use serde::{Deserialize, Serialize};

/// Stable identities used by the native host, never supplied as sender identity
/// by page code. Feature windows carry separate labels in the registry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WindowRole {
    Pet,
    CompactChat,
    FullChat,
    ChatBall,
    Subtitle,
    SubtitleSettings,
    AgentHud,
    Child,
}

impl WindowRole {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pet => "main",
            Self::CompactChat => "chat-compact",
            Self::FullChat => "chat-full",
            Self::ChatBall => "chat-ball",
            Self::Subtitle => "subtitle",
            Self::SubtitleSettings => "subtitle-settings",
            Self::AgentHud => "agent-hud",
            Self::Child => "child",
        }
    }

    pub(crate) fn route(self) -> Option<&'static str> {
        match self {
            Self::Pet => Some("/"),
            Self::CompactChat => Some("/chat"),
            Self::FullChat => Some("/chat_full"),
            Self::Subtitle | Self::SubtitleSettings => Some("/subtitle"),
            Self::AgentHud => Some("/agenthud"),
            Self::ChatBall | Self::Child => None,
        }
    }

    pub(crate) fn is_chat(self) -> bool {
        matches!(self, Self::CompactChat | Self::FullChat)
    }

    pub(crate) fn accepts_focus(self) -> bool {
        !matches!(self, Self::Subtitle)
    }

    pub(crate) fn decorated(self) -> bool {
        matches!(self, Self::Child | Self::SubtitleSettings)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChatSurface {
    #[default]
    Compact,
    Full,
}

impl ChatSurface {
    pub(crate) fn role(self) -> WindowRole {
        match self {
            Self::Compact => WindowRole::CompactChat,
            Self::Full => WindowRole::FullChat,
        }
    }
}
