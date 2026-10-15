//! Projects: logical folders that span devices. Requests name a project by id
//! or exact name, a device by endpoint id or label, and a session by name; the
//! app's answers are bounded so a large registry still fits one message.
use herdr_client::RepoKey;
use serde::{Deserialize, Serialize};

/// The longest project reference (an id or a name) a request carries, in bytes.
pub(crate) const MAX_PROJECT_REF: usize = 256;
/// The longest repository key or remote URL, in bytes.
pub(crate) const MAX_REPO: usize = 1024;
/// The longest bound folder, in bytes; the registry holds the same limit.
pub(crate) const MAX_PATH: usize = 1024;
/// The longest device reference (an endpoint id or a label), in bytes.
pub(crate) const MAX_DEVICE: usize = 512;
/// The longest session name, as Herdr itself allows.
pub(crate) const MAX_SESSION: usize = 64;
/// The most projects, and per project the most bindings, one list answer
/// names; `total` and `bindings_total` say what was left out.
#[cfg(any(unix, test))]
pub(crate) const MAX_LISTED_PROJECTS: usize = 200;
#[cfg(any(unix, test))]
pub(crate) const MAX_LISTED_BINDINGS: usize = 16;
/// A list answer stops adding projects once its text would pass this many
/// bytes, well inside the socket's message limit.
#[cfg(any(unix, test))]
pub(crate) const LIST_BUDGET: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ProjectOp {
    /// Every project with its bindings and assigned session count.
    List,
    /// A git project when `repo` is set (a normalized key), else a task one.
    Create {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo: Option<String>,
    },
    Rename {
        project: String,
        name: String,
    },
    /// Bind a folder on a device to a project. The device defaults to the one
    /// the caller's window is on.
    Bind {
        project: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
    },
    Unbind {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
    },
    /// Put a session in a project. The session defaults to the one the
    /// caller's window is attached to.
    Assign {
        project: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
    },
    Unassign {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
    },
    /// The project a session resolves to, and why.
    Show {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device: Option<String>,
    },
    /// Open the window's own delete-project confirmation. Never deletes.
    Delete {
        project: String,
    },
}

/// Whether `value` is a non-empty, printable text of at most `max` bytes.
fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn optional(value: &Option<String>, max: usize) -> bool {
    value.as_deref().is_none_or(|value| text(value, max))
}

pub(crate) fn valid_session(name: &str) -> bool {
    text(name, MAX_SESSION) && session_name(name)
}

fn session_ok(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(valid_session)
}

impl ProjectOp {
    /// Why a request is malformed, before anything is looked up. Every
    /// reference is echoed in answers, so none may hold a control character.
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        const REF: &str = "A project is named by an id or a name of 1 to 256 printable characters";
        const DEVICE: &str =
            "A device is named by an id or label of at most 512 printable characters";
        const SESSION: &str = "A session name is 1 to 64 letters, digits, '.', '_' or '-'";
        let check = |ok: bool, message: &'static str| ok.then_some(()).ok_or(message);
        let device = |value: &Option<String>| check(optional(value, MAX_DEVICE), DEVICE);
        let session = |value: &Option<String>| check(session_ok(value), SESSION);
        match self {
            Self::List => Ok(()),
            Self::Create { name, repo } => {
                check(text(name, MAX_PROJECT_REF), REF)?;
                check(
                    repo.as_deref()
                        .is_none_or(|repo| text(repo, MAX_REPO) && parse_repo(repo).is_some()),
                    "The repository must be a git remote URL or a host/owner/repo key",
                )
            }
            Self::Rename { project, name } => {
                check(text(project, MAX_PROJECT_REF), REF)?;
                check(text(name, MAX_PROJECT_REF), REF)
            }
            Self::Bind {
                project,
                path,
                device: on,
            } => {
                check(text(project, MAX_PROJECT_REF), REF)?;
                check(
                    text(path, MAX_PATH),
                    "A folder is 1 to 1024 printable bytes",
                )?;
                device(on)
            }
            Self::Unbind { path, device: on } => {
                check(
                    text(path, MAX_PATH),
                    "A folder is 1 to 1024 printable bytes",
                )?;
                device(on)
            }
            Self::Assign {
                project,
                session: named,
                device: on,
            } => {
                check(text(project, MAX_PROJECT_REF), REF)?;
                session(named)?;
                device(on)
            }
            Self::Unassign {
                session: named,
                device: on,
            }
            | Self::Show {
                session: named,
                device: on,
            } => {
                session(named)?;
                device(on)
            }
            Self::Delete { project } => check(text(project, MAX_PROJECT_REF), REF),
        }
    }
}

/// A repository from a remote URL or from a key (`host/owner/repo`), which has
/// no scheme: a key is read as the address of an `https` remote.
pub(crate) fn parse_repo(text: &str) -> Option<RepoKey> {
    let text = text.trim();
    RepoKey::from_remote_url(text).or_else(|| {
        let keyed = !text.contains([':', '\\']) && !text.starts_with(['/', '.']);
        keyed
            .then(|| RepoKey::from_remote_url(&format!("https://{text}")))
            .flatten()
    })
}

/// Whether a session name is one Herdr could have made.
fn session_name(name: &str) -> bool {
    !matches!(name, "." | "..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A folder of a project on one device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BindingInfo {
    pub device: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProjectInfo {
    pub id: String,
    pub name: String,
    /// The normalized repository, or none for a task project.
    pub repo: Option<String>,
    /// The first bindings, ordered by device then path.
    pub bindings: Vec<BindingInfo>,
    pub bindings_total: usize,
    /// Sessions explicitly assigned. Sessions that join by folder or
    /// repository are inferred from what each saved and are not counted.
    pub sessions: usize,
}

/// A project as `project show` names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProjectRef {
    pub id: String,
    pub name: String,
    pub repo: Option<String>,
}

/// What decided a session's project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SourceInfo {
    Assigned,
    /// A manual folder binding, as stored.
    Path {
        path: String,
    },
    /// The session's repository.
    Repository {
        repo: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Resolved {
    pub device: String,
    pub session: String,
    pub project: Option<ProjectRef>,
    pub source: Option<SourceInfo>,
    /// Why there is no project, or how far the answer can be trusted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Reply {
    Projects {
        projects: Vec<ProjectInfo>,
        /// Every project there is; more than `projects` when the list was cut.
        total: usize,
    },
    Project {
        project: ProjectInfo,
    },
    Resolved(Box<Resolved>),
    /// The delete-project confirmation is open in the window, awaiting the user.
    DeletePending {
        project: ProjectRef,
    },
}

#[cfg(test)]
mod tests;
