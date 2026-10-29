//! Windows, which are global to the app: list them, create, focus, rename or
//! close one, attach or detach a named session. Like all of this family it
//! never stops or deletes a Herdr session.
use serde::{Deserialize, Serialize};

#[cfg(any(unix, test))]
/// Longest device ID or session name a request may carry.
pub(crate) const MAX_NAME: usize = 256;
/// A list answer stops adding sessions once its text would pass this many
/// bytes, well inside the socket's message limit. Each Window gets an equal
/// share, so one full Window cannot hide the others' sessions.
#[cfg(unix)]
pub(crate) const LIST_BUDGET: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ContainerOp {
    /// Every Window, with the sessions each lists and which one this native
    /// window shows.
    List,
    /// A new, empty Window; this native window shows it.
    New {
        #[serde(default)]
        title: Option<String>,
    },
    /// Show the Window `window` (its ID or exact name) in this native window.
    Focus { window: String },
    /// Rename the Window `window` (its ID or exact name).
    Rename { window: String, title: String },
    /// Ask to close the Window `window` (its ID or exact name). Nothing closes
    /// yet: the app opens its close confirmation there, and the user confirms
    /// or cancels. Only an empty one closes; sessions are never touched.
    Close { window: String },
    /// Attach session `session` of `device` (default: the device the window
    /// shows) to `window` (default: the one this native window shows).
    Attach {
        session: String,
        #[serde(default)]
        device: Option<String>,
        #[serde(default)]
        window: Option<String>,
    },
    /// Move Window `window` to 1-based `position` in the list.
    Move { window: String, position: usize },
    /// Move a session to 1-based `position` in a Window's list.
    MoveSession {
        session: String,
        #[serde(default)]
        device: Option<String>,
        #[serde(default)]
        window: Option<String>,
        position: usize,
    },
    /// Record how the Window's sessions are to be grouped: `none`, `project`,
    /// `status` or `mr`. Stored for the sidebar; it changes no list.
    Group { window: String, by: String },
    /// Remove a session from a Window's list; the session keeps running.
    Detach {
        session: String,
        #[serde(default)]
        device: Option<String>,
        #[serde(default)]
        window: Option<String>,
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
    pub id: String,
    pub title: String,
    /// "none", "project", "status" or "mr"; stored, not yet rendered.
    pub grouping: String,
    /// Whether the addressed native window shows this Window.
    pub active: bool,
    pub sessions: Vec<SlotInfo>,
    /// How many sessions the Window lists; `sessions` holds fewer when the
    /// answer was cut to fit `LIST_BUDGET`.
    pub sessions_total: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WindowRef {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Reply {
    Containers {
        containers: Vec<ContainerInfo>,
    },
    /// The close confirmation is open in the window, awaiting the user.
    ClosePending {
        window: WindowRef,
    },
}

#[cfg(any(unix, test))]
/// A Window reference (ID or name), a device ID or a session name a request may carry: printable and bounded.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control)
}
