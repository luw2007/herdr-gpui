//! Logical Windows inside one native window: list them, create or focus one,
//! attach a named session. Removing or closing is a UI action and, like all of
//! this family, never stops or deletes a Herdr session.
use serde::{Deserialize, Serialize};

/// Longest device ID or session name a request may carry.
pub(crate) const MAX_NAME: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ContainerOp {
    /// Every logical Window of the window, with the sessions each lists.
    List,
    /// A new, empty logical Window; it becomes the active one.
    New {
        #[serde(default)]
        title: Option<String>,
    },
    /// Show the logical Window with this ID.
    Focus { id: u64 },
    /// Attach session `session` of `device` (default: the device the window
    /// shows) to the active logical Window, and show it.
    Attach {
        session: String,
        #[serde(default)]
        device: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SlotInfo {
    pub device: String,
    pub session: String,
    pub selected: bool,
    /// "online", "connecting", "detached", ... as `host list` reports.
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContainerInfo {
    pub id: u64,
    pub title: String,
    pub active: bool,
    pub sessions: Vec<SlotInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Reply {
    Containers { containers: Vec<ContainerInfo> },
}

/// A device ID or session name a request may carry: printable and bounded.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control)
}
