//! Automatic project discovery from saved workspaces.
//!
//! A session the registry cannot already place — no assignment, no covering
//! manual binding, no project for a workspace repository — is provisioned by
//! [`Registry::discover`]: the first workspace with a usable identity wins.
//! With a repository remote, that repository's project is created or reused,
//! so clones on every device join without setup. Without one, the workspace's
//! common root (its saved `project`, else its own path) becomes an inferred
//! path binding: a device-local entry in a map kept apart from the manual
//! bindings, so the user's choices remain theirs, resolve above the
//! inference, and a later manual binding of the same folder never has to
//! fight a guess.
//!
//! Discovered identities are not random. A project id is UUIDv5 (RFC 4122)
//! of the repository key or of `(device, path)` under a fixed namespace, so
//! two windows discovering the same thing from empty registries derive the
//! same id — and, because names always carry a short id suffix
//! (`app · 0f3f22ab`), the same name — and merge into one project instead of
//! colliding. Deleting a discovered project is safe: the next scan recreates
//! it from the same workspaces.
use super::{
    DeviceKey, Limit, MAX_BINDINGS, MAX_PROJECTS, Project, ProjectError, ProjectId, Registry,
    RepoKey, SessionRef,
    types::{NAME_MAX, components, normalize_path, valid_name},
};
use crate::SessionWorkspace;
use uuid::Uuid;

/// A fixed UUIDv5 namespace for discovered identities. Only stability
/// matters, so the value is an arbitrary constant of this crate.
const DISCOVERED_NAMESPACE: Uuid = Uuid::from_u128(0x6865_6472_5F70_726F_6A5F_6469_7363_6F76);

/// The name suffixes tried in order (8, then 12, then 32 hex): enough to step
/// around a user project that already spells the generated name, few enough
/// to stay bounded. All derive from the identity itself, so every window
/// proposing a name for the same identity proposes the same one.
const SUFFIX_HEX: [usize; 3] = [8, 12, 32];
const SEPARATOR: &str = " · ";

/// The stable id of a discovered identity: `kind` separates the repository
/// and path namespaces; `material` is the repository key or `device␀path`.
fn stable_id(kind: &str, material: &str) -> ProjectId {
    ProjectId::from_uuid(Uuid::new_v5(
        &DISCOVERED_NAMESPACE,
        format!("{kind}\u{0}{material}").as_bytes(),
    ))
}

/// The human name of a repository: the last segment of its key
/// (`github.com/owner/repo` names `repo`).
fn repo_tail(repo: &RepoKey) -> Option<&str> {
    let tail = repo.as_str().rsplit('/').next()?;
    (!tail.is_empty()).then_some(tail)
}

/// The basename of a fallback root (`/home/u/proj` names `proj`).
fn path_basename(root: &str) -> Option<String> {
    components(root).pop().map(str::to_owned)
}

/// The folder a workspace without a repository infers: its saved common root
/// (`project`) if usable, else its own path. Absolute, normalized exactly
/// like a stored binding, deeper than the file system root (a root would
/// group a whole machine or drive, and a UNC server also needs its share),
/// and free of `.`/`..` segments. Anything else is skipped rather than
/// guessed into a project.
fn fallback_root(workspace: &SessionWorkspace) -> Option<String> {
    for candidate in [workspace.project.as_str(), workspace.path.as_str()] {
        if candidate.is_empty() {
            continue;
        }
        let Ok(root) = normalize_path(candidate) else {
            continue;
        };
        let parts = components(&root);
        let needed = if root.starts_with("\\\\") { 3 } else { 2 };
        if parts.len() >= needed && !parts[1..].iter().any(|part| *part == "." || *part == "..") {
            return Some(root);
        }
    }
    None
}

impl Registry {
    /// Provision projects for one session from its saved workspaces. See
    /// [`Registry::discover_sessions`] for the semantics; the return says
    /// whether anything changed.
    pub fn discover(
        &mut self,
        session: &SessionRef,
        workspaces: &[SessionWorkspace],
    ) -> Result<bool, ProjectError> {
        self.discover_sessions(std::iter::once((session, workspaces)))
    }

    /// The same for many sessions: the registry is cloned once and committed
    /// only if something changed, so one failure anywhere leaves it
    /// untouched. A session the registry already resolves — an explicit
    /// assignment, a covering manual binding, or a project for one of its
    /// workspace repositories — is skipped: whatever the user chose wins.
    /// Otherwise the first workspace with a usable identity wins: its
    /// repository's project is created or reused, or, without a repository,
    /// an inferred path binding records the workspace's root on this device.
    pub fn discover_sessions<'a, I>(&mut self, sessions: I) -> Result<bool, ProjectError>
    where
        I: IntoIterator<Item = (&'a SessionRef, &'a [SessionWorkspace])>,
    {
        let mut staged = self.clone();
        let mut changed = false;
        for (session, workspaces) in sessions {
            changed |= staged.discover_one(session, workspaces)?;
        }
        if changed {
            *self = staged;
        }
        Ok(changed)
    }

    fn discover_one(
        &mut self,
        session: &SessionRef,
        workspaces: &[SessionWorkspace],
    ) -> Result<bool, ProjectError> {
        if self.resolve(session, workspaces).project.is_some() {
            return Ok(false);
        }
        for workspace in workspaces {
            if let Some(repo) = &workspace.repo {
                return self.discover_repo(repo);
            }
            if let Some(root) = fallback_root(workspace) {
                return self.discover_path(&session.device, root);
            }
            // An unusable context is skipped, not invented into a project; a
            // later workspace may still carry one.
        }
        Ok(false)
    }

    /// Create (or find) the project of a repository no project tracks yet.
    fn discover_repo(&mut self, repo: &RepoKey) -> Result<bool, ProjectError> {
        let id = stable_id("repo", repo.as_str());
        if self.projects.contains_key(&id) {
            // The identity already exists: nothing to create. resolve would
            // already have placed the session here, so this branch is only
            // reachable through a hash collision or a renamed project; report
            // no change rather than pretend one happened.
            return Ok(false);
        }
        let Some(name) = self.free_name(repo_tail(repo), &id) else {
            return Ok(false);
        };
        self.insert_discovered(id, name, Some(repo.clone()))?;
        Ok(true)
    }

    /// Create (or find) the project `root` on `device` infers, and record the
    /// inference in the inferred map — never among the manual bindings.
    fn discover_path(&mut self, device: &DeviceKey, root: String) -> Result<bool, ProjectError> {
        if self.inferred.contains_key(&(device.clone(), root.clone())) {
            return Ok(false);
        }
        let id = stable_id("path", &format!("{device}\u{0}{root}"));
        match self.projects.get(&id).map(|project| project.repo.is_none()) {
            // The project is already there (merged in, or its inference was
            // dropped): only re-record the inference.
            Some(true) => {}
            // The id belongs to a repository project: not this identity.
            Some(false) => return Ok(false),
            None => {
                let Some(name) = self.free_name(path_basename(&root).as_deref(), &id) else {
                    return Ok(false);
                };
                self.insert_discovered(id.clone(), name, None)?;
            }
        }
        // Manual and inferred path bindings share one cap.
        if self.bindings.len() + self.inferred.len() >= MAX_BINDINGS {
            return Err(ProjectError::LimitReached(Limit::Bindings));
        }
        self.inferred.insert((device.clone(), root), id);
        Ok(true)
    }

    /// The generated name of the identity `id` from `base`: the first
    /// `base · <hex>` spelling no existing project uses (case-insensitively),
    /// where `<hex>` is the id's leading hex digits without separators. `None`
    /// when `base` names nothing or every suffix length is taken.
    fn free_name(&self, base: Option<&str>, id: &ProjectId) -> Option<String> {
        let base = base?;
        let hex: Vec<char> = id.as_str().chars().filter(|c| *c != '-').collect();
        for suffix in SUFFIX_HEX {
            let room = NAME_MAX - suffix - SEPARATOR.chars().count();
            let Ok(base) = valid_name(&base.chars().take(room).collect::<String>()) else {
                return None;
            };
            let tail: String = hex[..suffix].iter().collect();
            let candidate = format!("{base}{SEPARATOR}{tail}");
            if self.check_name(&candidate, None).is_ok() {
                return Some(candidate);
            }
        }
        None
    }

    /// Insert a discovered project under its predetermined id; the caller
    /// already freed the name.
    fn insert_discovered(
        &mut self,
        id: ProjectId,
        name: String,
        repo: Option<RepoKey>,
    ) -> Result<(), ProjectError> {
        if self.projects.len() >= MAX_PROJECTS {
            return Err(ProjectError::LimitReached(Limit::Projects));
        }
        self.projects.insert(id.clone(), Project { id, name, repo });
        Ok(())
    }
}
