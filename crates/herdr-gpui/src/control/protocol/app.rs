//! The app, its windows, and their chrome.
use serde::{Deserialize, Serialize};

/// `on`, `off`, or `toggle`, as several settings accept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Switch {
    On,
    Off,
    #[default]
    Toggle,
}

impl Switch {
    /// The state after applying this to `current`.
    #[cfg(unix)]
    pub fn apply(self, current: bool) -> bool {
        match self {
            Self::On => true,
            Self::Off => false,
            Self::Toggle => !current,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AppOp {
    /// Bring the app and its window forward.
    Focus,
    Hide,
    /// Quit the app; Herdr and its terminals keep running.
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum WindowOp {
    List,
    /// Another window of the first window's launch target, or of a named
    /// local session.
    New {
        #[serde(default)]
        session: Option<String>,
    },
    Focus,
    Fullscreen {
        switch: Switch,
    },
    /// Reports the window's size, or resizes its content when given one.
    /// GPUI cannot move a window, so there is no position to set.
    Size {
        #[serde(default)]
        size: Option<Size>,
    },
    /// Detach from the selected host; it keeps running.
    Detach,
    Reconnect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case", deny_unknown_fields)]
pub enum FontSize {
    Larger,
    Smaller,
    /// Back to the size the config file sets.
    Reset,
    Points {
        points: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum UiOp {
    Sidebar { switch: Switch },
    StatusBar { switch: Switch },
    FontSize { size: FontSize },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CommandOp {
    /// A command by its `[keybindings]` name, as the palette runs it.
    Run { name: String },
    /// Opens the app's Sessions manager with a project-index query and the
    /// optional Device and Project filters (a device or project reference;
    /// `unassigned` is the explicit "No project" choice).
    Sessions {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        unassigned: bool,
    },
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Queries may contain spaces and paths, but stay bounded and printable.
pub(crate) const MAX_SESSION_QUERY: usize = 4096;

pub(crate) fn valid_session_query(query: &str) -> bool {
    query.len() <= MAX_SESSION_QUERY && !query.chars().any(char::is_control)
}
/// What `status` reports about the running app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Status {
    pub version: String,
    pub protocol: u32,
    pub pid: u32,
    /// Whether pages show in tabs; otherwise they open in the system browser.
    pub browser_tabs: bool,
    pub windows: Vec<WindowInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct WindowInfo {
    pub id: u64,
    pub title: String,
    pub active: bool,
    pub fullscreen: bool,
    pub width: f32,
    pub height: f32,
    /// The workspace the window shows on its selected host.
    pub workspace: Option<String>,
    pub hosts: Vec<HostInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HostInfo {
    pub id: String,
    pub label: String,
    /// "online", "connecting", "reconnecting", "detached", "disabled", ...
    pub status: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Reply {
    Windows { windows: Vec<WindowInfo> },
    Size { width: f32, height: f32 },
    FontSize { points: f32 },
}

#[cfg(test)]
mod tests;
