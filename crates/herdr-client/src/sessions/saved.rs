//! What a session's own `session.json` says about the workspaces it last saved.
//! This is the file Herdr rewrites as a session changes, so it names the real
//! project even for a session whose name is only a hash, and it is readable
//! while the daemon is stopped. It can lag the live session: callers must show
//! `project`/`path`/`label` as saved context, not as current activity. `branch`
//! and, for ungrouped workspaces, `project` are refined by `git` probes.
//!
//! Everything here is bounded and per-session: a file that is too large, not
//! JSON, or shaped differently yields no workspaces for that one session and
//! never fails a listing.
use super::{LIMIT, SessionList};
use crate::discovery::valid_session_name;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    path::Path,
};

/// The most bytes of one `session.json` that are read or sent over SSH. Real
/// snapshots are a few KiB; a larger file is left unclassified, not truncated
/// into invalid JSON.
pub(crate) const SNAPSHOT_MAX: usize = 256 * 1024;

/// Workspaces kept per session.
const WORKSPACES_MAX: usize = 64;

/// Longest accepted path, label, or name.
const TEXT_MAX: usize = 1024;

/// Remote framing: `BEGIN<name>`, base64 lines, then `END`. `DONE` closes the
/// whole output, so a reader knows no more records follow.
pub(crate) const BEGIN: &str = "herdr-saved:begin:";
const END: &str = "herdr-saved:end";
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const DONE: &str = "herdr-saved:done";

/// Base64 of a maximal snapshot, plus line breaks.
const ENCODED_MAX: usize = SNAPSHOT_MAX / 3 * 4 + 4096;

/// One workspace a session last saved, as context for finding the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionWorkspace {
    /// The repository or project the workspace belongs to: the saved worktree
    /// group's repository root, else git's common repository directory for the
    /// workspace path, else the workspace's own directory.
    pub project: String,
    /// The workspace's checkout or working directory.
    pub path: String,
    /// The branch checked out in the workspace when last probed with a
    /// read-only `git symbolic-ref`, else one the saved file states, else
    /// `None` (detached HEAD, not a repository, or git unavailable). A live
    /// probe may be cached for about 30 seconds.
    pub branch: Option<String>,
    /// The workspace's saved name or worktree-group label.
    pub label: Option<String>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty() && text.len() <= TEXT_MAX && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}

/// The workspaces in a `session.json`, or none when the bytes are not one.
pub(crate) fn parse_snapshot(bytes: &[u8]) -> Vec<SessionWorkspace> {
    if bytes.len() > SNAPSHOT_MAX {
        return Vec::new();
    }
    let Ok(snapshot) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    let Some(listed) = snapshot.get("workspaces").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut workspaces: Vec<SessionWorkspace> = Vec::new();
    for workspace in listed.iter().take(WORKSPACES_MAX) {
        let space = workspace.get("worktree_space").unwrap_or(&Value::Null);
        let cwd = text(workspace, "identity_cwd");
        let path = text(space, "checkout_path").or_else(|| cwd.clone());
        let project = text(space, "repo_root").or(cwd).or_else(|| path.clone());
        let (Some(project), Some(path)) = (project, path.or_else(|| text(space, "repo_root")))
        else {
            continue;
        };
        if workspaces
            .iter()
            .any(|known| known.project == project && known.path == path)
        {
            continue;
        }
        workspaces.push(SessionWorkspace {
            project,
            path,
            branch: text(workspace, "branch").or_else(|| text(space, "branch")),
            label: text(workspace, "custom_name").or_else(|| text(space, "label")),
        });
    }
    workspaces
}

/// A local session's saved workspaces. Symlinks, non-files, oversized, and
/// unreadable files all mean "unknown".
pub(crate) fn read_snapshot(session_dir: &Path) -> Vec<SessionWorkspace> {
    read_bounded(&session_dir.join("session.json"))
        .map(|bytes| parse_snapshot(&bytes))
        .unwrap_or_default()
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    // `symlink_metadata` does not follow: a link out of the session directory
    // is never read.
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(SNAPSHOT_MAX as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Everything a remote listing printed, classified line by line.
pub(crate) struct Scan {
    pub list: Option<SessionList>,
    pub invalid: Option<serde_json::Error>,
    pub saved: HashMap<String, Vec<SessionWorkspace>>,
}

struct Record {
    name: Option<String>,
    encoded: Vec<u8>,
    overflow: bool,
}

/// Split a remote output into the host's JSON listing and its snapshot records.
/// The last JSON line that parses is the listing; a record that is unterminated,
/// oversized, or undecodable is dropped alone.
pub(crate) fn scan(output: &[u8]) -> Scan {
    let mut scan = Scan {
        list: None,
        invalid: None,
        saved: HashMap::new(),
    };
    let mut record: Option<Record> = None;
    for line in output.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix(BEGIN.as_bytes()) {
            record = Some(Record {
                name: std::str::from_utf8(name)
                    .ok()
                    .filter(|name| valid_session_name(name))
                    .map(str::to_owned),
                encoded: Vec::new(),
                overflow: false,
            });
        } else if line == END.as_bytes() {
            if let Some(done) = record.take()
                && let Some(name) = done.name
                && !done.overflow
                && scan.saved.len() < LIMIT
                && let Ok(bytes) = STANDARD.decode(&done.encoded)
            {
                scan.saved.insert(name, parse_snapshot(&bytes));
            }
        } else if let Some(open) = record.as_mut() {
            if open.encoded.len() + line.len() > ENCODED_MAX {
                open.overflow = true;
            } else {
                open.encoded.extend_from_slice(line);
            }
        } else {
            match serde_json::from_slice::<SessionList>(line) {
                Ok(list) => scan.list = Some(list),
                Err(error) => scan.invalid = Some(error),
            }
        }
    }
    scan
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
