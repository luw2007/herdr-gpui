//! Lets processes on this machine, such as an agent in a Herdr pane, drive
//! the parts of the app Herdr cannot: show the user a page and hear back
//! what the user noted on it, open the app's own tabs, and read or change
//! state only this client keeps. Herdr itself has no way to reach a client,
//! so the app listens on a socket of its own and `herdr-gpui <command> ...`
//! talks to it. Only this user's local processes can reach it: an agent on
//! an SSH host has no path to this machine's socket.

mod client;
mod protocol;
#[cfg(unix)]
mod server;
#[cfg(unix)]
mod socket;

#[cfg(unix)]
use crate::HerdrWindow;
#[cfg(unix)]
use crate::browser::Location;
#[cfg(unix)]
pub(crate) use crate::cli::control::Placement;
pub(crate) use client::run;
#[cfg(feature = "mockup")]
pub(crate) use client::send_notes;
#[cfg(unix)]
use gpui::App;
#[cfg(unix)]
use protocol::Caller;
pub use protocol::ErrorCode;
#[cfg(any(unix, feature = "mockup"))]
pub(crate) use protocol::NotesTo;
pub(crate) use protocol::app::valid_session_query;
pub use protocol::{
    app::{FontSize, Size, Switch},
    events::{Cursor, EventKind},
    settings::Sound,
    work::Script,
};
#[cfg(unix)]
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", windows))]
/// The command that reloads a caller's pages, for the notes prompt.
pub(crate) fn reload_command() -> String {
    static COMMAND: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    COMMAND
        .get_or_init(|| {
            format!(
                "{} browser reload",
                crate::agent_skill::command(std::env::current_exe().ok().as_deref())
            )
        })
        .clone()
}

#[cfg(unix)]
fn socket_path() -> Option<PathBuf> {
    crate::preferences::state_dir().map(|dir| dir.join("control.sock"))
}

/// What a request asks the windows to find: a workspace, optionally of one
/// particular local daemon, and the pane asking.
#[cfg(unix)]
pub(crate) struct Target<'a> {
    pub daemon: Option<&'a Path>,
    pub workspace: Option<&'a str>,
    pub pane: Option<&'a str>,
}

#[cfg(unix)]
impl Target<'_> {
    /// How strictly a request's workspace is matched, strict first. A caller
    /// that names its daemon is matched on that daemon only: another
    /// session's workspace of the same ID must never be changed for it.
    pub(crate) fn strictness(&self) -> &'static [bool] {
        if self.daemon.is_some() {
            &[true]
        } else {
            &[true, false]
        }
    }
}

/// The page a request asks a window to open, and how to show it.
#[cfg(unix)]
pub(crate) struct Opening<'a> {
    pub location: &'a Location,
    /// Whether the window switches to the tab.
    pub focus: bool,
    pub placement: Placement,
}

/// Where a window opened a requested tab.
#[cfg(unix)]
pub(crate) enum Placed {
    Opened {
        workspace_id: String,
        placement: Placement,
    },
    Full,
}

/// Identifiers a request carries are compared, never interpreted, but they
/// are echoed in answers, so they stay short and printable.
#[cfg(any(unix, test))]
fn plain(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.chars().all(|c| !c.is_control() && !c.is_whitespace())
}

#[cfg(unix)]
fn valid_caller(caller: &Caller) -> bool {
    caller
        .workspace_id
        .as_deref()
        .is_none_or(|id| plain(id, 256))
        && caller.pane_id.as_deref().is_none_or(|id| plain(id, 256))
        && caller
            .daemon_socket
            .as_deref()
            .is_none_or(|path| plain(path, 4096))
}

#[cfg(unix)]
fn main_windows(cx: &App) -> Vec<gpui::WindowHandle<HerdrWindow>> {
    let active = cx.active_window();
    let mut windows: Vec<_> = cx
        .windows()
        .into_iter()
        .filter_map(|handle| handle.downcast::<HerdrWindow>())
        .collect();
    // The frontmost window wins a workspace that several windows show.
    windows.sort_by_key(|handle| Some(handle.window_id()) != active.map(|a| a.window_id()));
    windows
}

#[cfg(unix)]
pub(crate) use server::install;

#[cfg(not(unix))]
pub(crate) fn install(_: &mut gpui::App) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echoed_identifiers_are_short_and_printable() {
        assert!(plain("w_1", 256));
        for invalid in ["", "w 1", "w\u{1b}[2J", "w\n1"] {
            assert!(!plain(invalid, 256), "{invalid:?}");
        }
        assert!(!plain(&"w".repeat(257), 256));
    }
}
