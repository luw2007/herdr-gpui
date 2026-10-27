//! Projects: logical folders that span machines.
//!
//! A project is a git repository (identified by its normalized remote, so
//! clones on different devices join without any setup) or a task codename
//! without git (joined to each device by a path binding). A Herdr session
//! belongs to at most one project, assigned explicitly or inferred from its
//! saved workspaces; it may belong to none. [`Registry::discover`] turns that
//! inference into state: unbound workspaces get their repository's project or
//! a device-local inferred path, never overriding a manual choice. The
//! registry is plain GUI-local state: pure data and a JSON codec, no file
//! access, no protocol.
mod codec;
mod discover;
mod error;
mod merge;
mod registry;
mod repo_key;
mod resolve;
mod types;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

pub use error::{Limit, ProjectError};
pub use registry::Registry;
pub use repo_key::RepoKey;
pub use resolve::{Resolution, Source};
pub use types::{Binding, DeviceKey, Project, ProjectId, SessionRef};

pub(crate) const MAX_PROJECTS: usize = 1024;
pub(crate) const MAX_BINDINGS: usize = 8192;
pub(crate) const MAX_ASSIGNMENTS: usize = 8192;
