//! Typed failures of the project registry and its JSON codec.
//!
//! Messages never echo registry contents beyond a project name or id the
//! caller just supplied; codec failures keep the `serde_json` error as a
//! source only, because its text can quote file content.
use super::{DeviceKey, ProjectId};

/// The collection whose size cap was hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Projects,
    Bindings,
    Assignments,
}

impl std::fmt::Display for Limit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Projects => write!(f, "projects (at most {})", super::MAX_PROJECTS),
            Self::Bindings => write!(f, "path bindings (at most {})", super::MAX_BINDINGS),
            Self::Assignments => {
                write!(
                    f,
                    "session assignments (at most {})",
                    super::MAX_ASSIGNMENTS
                )
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("a project name must be 1 to 64 characters without control characters")]
    InvalidName,
    #[error("a project named \"{0}\" already exists")]
    DuplicateName(String),
    #[error("another project already tracks this repository")]
    DuplicateRepo { existing: ProjectId },
    #[error("unknown project {0}")]
    UnknownProject(ProjectId),
    #[error("a project id must be a lowercase hyphenated UUID")]
    InvalidId,
    #[error("a device key must be 1 to 128 characters without control characters")]
    InvalidDevice,
    #[error("invalid session name")]
    InvalidSession,
    #[error("a bound path must be absolute, at most 1024 bytes, and free of control characters")]
    InvalidPath,
    #[error("a path on {device} is already bound to another project")]
    BindingConflict {
        device: DeviceKey,
        existing: ProjectId,
    },
    #[error("too many {0}")]
    LimitReached(Limit),
    #[error("unsupported project registry version {0}")]
    UnsupportedVersion(u32),
    #[error("project registry exceeds its size limit")]
    TooLarge,
    #[error("project registry repeats an entry")]
    DuplicateEntry,
    #[error("invalid project registry schema")]
    Schema(#[source] serde_json::Error),
}
