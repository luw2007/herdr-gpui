//! Identifiers and records of the project registry, each validated at the
//! point it is built so a registry never holds a value the codec would refuse.
use super::{ProjectError, RepoKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use uuid::Uuid;

const NAME_MAX: usize = 64;
const DEVICE_MAX: usize = 128;
pub(crate) const PATH_MAX: usize = 1024;

/// A project's stable identity: a random (v4) UUID in lowercase hyphenated
/// form. Names, repositories and paths can change; the id never does.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectId(String);

impl ProjectId {
    pub(crate) fn generate() -> Self {
        Self(Uuid::new_v4().hyphenated().to_string())
    }

    /// An id read back from storage or a command line.
    pub fn parse(id: &str) -> Result<Self, ProjectError> {
        match Uuid::parse_str(id) {
            Ok(uuid) if uuid.hyphenated().to_string() == id => Ok(Self(id.to_owned())),
            _ => Err(ProjectError::InvalidId),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ProjectId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProjectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = String::deserialize(deserializer)?;
        Self::parse(&id).map_err(serde::de::Error::custom)
    }
}

/// An opaque GUI endpoint id: `local`, `ssh:<profile-id>`, `wsl:<distro>`, or
/// a cloud endpoint id. The registry never interprets it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceKey(String);

impl DeviceKey {
    /// Fails for an empty key, one over 128 characters, or one with controls.
    pub fn new(key: impl Into<String>) -> Result<Self, ProjectError> {
        let key = key.into();
        if key.is_empty() || key.chars().count() > DEVICE_MAX || key.chars().any(char::is_control) {
            return Err(ProjectError::InvalidDevice);
        }
        Ok(Self(key))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for DeviceKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DeviceKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A Herdr session on one device. Never a path, branch, label, or the hashed
/// session name's meaning: only the pair identifies a session.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionRef {
    pub device: DeviceKey,
    pub session: String,
}

impl SessionRef {
    pub fn new(device: DeviceKey, session: impl Into<String>) -> Self {
        Self {
            device,
            session: session.into(),
        }
    }
}

/// A logical folder across machines: a git repository (`repo` is `Some`) or a
/// task codename without git (`repo` is `None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub repo: Option<RepoKey>,
}

/// A manual per-device folder of a project. It covers `path` and everything
/// below it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub device: DeviceKey,
    pub path: String,
    pub project: ProjectId,
}

/// A project name: trimmed, 1 to 64 characters, no control characters.
pub(crate) fn valid_name(name: &str) -> Result<String, ProjectError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX || name.chars().any(char::is_control) {
        return Err(ProjectError::InvalidName);
    }
    Ok(name.to_owned())
}

fn drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

/// The stored form of a bound path: absolute (`/…`, `X:\…`, `X:/…`, or UNC
/// `\\…`), at most 1024 bytes, no control characters, trailing separators
/// trimmed but never below the root (`/` and `C:\` stay).
pub(crate) fn normalize_path(path: &str) -> Result<String, ProjectError> {
    if path.is_empty() || path.len() > PATH_MAX || path.chars().any(char::is_control) {
        return Err(ProjectError::InvalidPath);
    }
    let unc = path.starts_with("\\\\");
    if !(path.starts_with('/') || unc || drive_root(path)) {
        return Err(ProjectError::InvalidPath);
    }
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        // Only separators: a POSIX root, never a bare UNC prefix.
        return if path.starts_with('/') {
            Ok("/".to_owned())
        } else {
            Err(ProjectError::InvalidPath)
        };
    }
    if trimmed.len() == 2 && drive_root(path) {
        return Ok(path[..3].to_owned());
    }
    Ok(trimmed.to_owned())
}

/// A path as comparable components. `/` and `\` both separate; empty segments
/// are dropped; segments compare exactly (case-sensitive, no `.`/`..`
/// resolution, since the registry never sees the file system). A leading
/// separator becomes a root marker so `/a` and `a` never match, and UNC
/// (`\\`) has its own marker.
pub(crate) fn components(path: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    if path.starts_with("\\\\") {
        parts.push("\\\\");
    } else if path.starts_with(['/', '\\']) {
        parts.push("/");
    }
    parts.extend(path.split(['/', '\\']).filter(|part| !part.is_empty()));
    parts
}
