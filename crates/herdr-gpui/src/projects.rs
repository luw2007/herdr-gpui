//! The app's projects: logical folders that span machines, shared by every
//! window like the browser tabs. The registry lives in `projects.json` in the
//! state directory and is GUI-local; no daemon or protocol carries it.
//!
//! Loading never fails the app. A missing file is an empty registry. A file
//! that is corrupt, oversized or of an unsupported version also starts empty,
//! with a warning, and is left untouched until the user changes something; the
//! first save then renames it to `projects.json.bad` (replacing an older
//! backup) before writing, so a bad file is never silently destroyed.
//!
//! Saving hands a snapshot to a [`state_file::Writer`] worker: the UI thread
//! never waits on the disk. Every mutation bumps [`Projects::revision`] and
//! notifies the global's observers, which is how every window repaints.
use crate::{
    shared_state::{self, Shared},
    state_file,
};
use gpui::{App, BorrowAppContext, Global};
use herdr_client::{
    Binding, DeviceKey, Project, ProjectError, ProjectId, Registry, RepoKey, Resolution,
    SessionRef, SessionWorkspace,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(test)]
mod tests;

const FILE: &str = "projects.json";
const BACKUP: &str = "projects.json.bad";

impl Shared for Registry {
    const LIMIT: u64 = Registry::MAX_JSON_BYTES as u64;
    fn decode(bytes: &[u8]) -> crate::Result<Self> {
        Ok(Registry::from_json(bytes)?)
    }
    fn encode(&self) -> crate::Result<Vec<u8>> {
        Ok(self.to_json()?)
    }
    fn merge(base: &Self, mine: &Self, theirs: &Self) -> crate::Result<Self> {
        Ok(Registry::merge(base, mine, theirs)?)
    }
}
type Snapshot = shared_state::Snapshot<Registry>;

#[derive(Default)]
pub(crate) struct Projects {
    registry: Registry,
    revision: u64,
    writer: Option<state_file::Writer<Snapshot>>,
    quitting: bool,
    /// Saves that picked up another process's edits, waiting to be adopted.
    inbox: Arc<shared_state::Inbox<Registry>>,
}

impl Global for Projects {}

impl Projects {
    fn path() -> Option<PathBuf> {
        crate::preferences::state_dir().map(|dir| dir.join(FILE))
    }

    /// Called before starting GPUI. See the module comment for the policy on
    /// a missing or unreadable file.
    pub(crate) fn load() -> Self {
        Self::at(Self::path())
    }

    pub(crate) fn at(path: Option<PathBuf>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let registry = match read_registry(&path) {
            Ok(registry) => registry,
            Err(error) => {
                tracing::warn!(%error, "Cannot restore projects; starting empty and keeping the file");
                Registry::new()
            }
        };
        let backup = Some(backup_path(&path));
        let inbox = Arc::new(shared_state::Inbox::default());
        let save = shared_state::saver(registry.clone(), backup.clone(), inbox.clone());
        let writer = match state_file::Writer::start_with("projects", path, None, save) {
            Ok(writer) => Some(writer),
            Err(error) => {
                tracing::warn!(%error, "Cannot start the projects worker");
                None
            }
        };
        Self {
            registry,
            revision: 0,
            writer,
            quitting: false,
            inbox,
        }
    }

    pub(crate) fn install(self, cx: &mut App) {
        cx.set_global(self);
        cx.on_app_quit(|cx| {
            let store = cx.global_mut::<Self>();
            store.quitting = true;
            let writer = store.writer.take();
            cx.background_executor().spawn(async move {
                if let Some(writer) = writer {
                    writer.finish();
                }
            })
        })
        .detach();
    }

    /// Runs `f` against the app's store, creating an unsaved one for fixtures
    /// that never installed it. Observers of the global are notified.
    pub(crate) fn update<R>(cx: &mut App, f: impl FnOnce(&mut Self) -> R) -> R {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        cx.update_global::<Self, R>(|store, _| f(store))
    }

    /// A read-only view, or an empty registry when none was installed.
    pub(crate) fn read<R>(cx: &App, f: impl FnOnce(&Registry) -> R) -> R {
        match cx.try_global::<Self>() {
            Some(store) => f(&store.registry),
            None => f(&Registry::new()),
        }
    }

    /// Increases by one on every successful mutation, so a view can tell its
    /// cached grouping is stale without comparing registries.
    #[cfg(test)]
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    // The control handlers (CLI/socket) call this; the GUI itself does not.
    #[allow(dead_code)]
    pub(crate) fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Applies `change`; on success counts the revision and queues a save.
    /// A failed change, including one that would make the registry too large
    /// to save, leaves the registry as it was.
    fn mutate<R>(
        &mut self,
        change: impl FnOnce(&mut Registry) -> Result<R, ProjectError>,
    ) -> crate::Result<R> {
        let mut next = self.registry.clone();
        let result = change(&mut next)?;
        if next != self.registry {
            // Refuse a state the writer could not persist, so a success here
            // always means the change survives a restart. The check counts the
            // encoding up to the 4 MiB cap without allocating it, the same
            // order of work as the clone above.
            next.check_saveable()?;
            self.registry = next;
            self.revision += 1;
            self.save();
        }
        Ok(result)
    }

    /// Whether a save merged in another process's edits not adopted yet.
    pub(crate) fn has_external_edits(&self) -> bool {
        self.inbox.has_news()
    }
    /// Adopts what another process saved, keeping any edit made since.
    pub(crate) fn adopt_external(&mut self) {
        match shared_state::adopt(&self.inbox, &self.registry) {
            Ok(Some((next, dirty))) => {
                self.registry = next;
                self.revision += 1;
                if dirty {
                    self.save();
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "Cannot merge another process's projects"),
        }
    }
    fn save(&self) {
        if self.quitting {
            return;
        }
        if let Some(writer) = &self.writer {
            writer.save(shared_state::Snapshot(self.registry.clone()));
        }
    }

    pub(crate) fn create_project(
        &mut self,
        name: &str,
        repo: Option<RepoKey>,
    ) -> crate::Result<Project> {
        self.mutate(|registry| registry.create_project(name, repo))
    }

    pub(crate) fn rename_project(&mut self, id: &ProjectId, name: &str) -> crate::Result<()> {
        self.mutate(|registry| registry.rename_project(id, name))
    }

    pub(crate) fn remove_project(&mut self, id: &ProjectId) -> crate::Result<Project> {
        self.mutate(|registry| registry.remove_project(id))
    }

    #[cfg(any(unix, test))]
    pub(crate) fn bind(
        &mut self,
        device: &str,
        path: &str,
        project: &ProjectId,
    ) -> crate::Result<Binding> {
        let device = DeviceKey::new(device)?;
        self.mutate(|registry| registry.bind(device, path, project))
    }

    /// Whether a binding was removed.
    // The control handlers (CLI/socket) call this; the GUI itself does not.
    #[allow(dead_code)]
    pub(crate) fn unbind(&mut self, device: &str, path: &str) -> crate::Result<bool> {
        let device = DeviceKey::new(device)?;
        self.mutate(|registry| registry.unbind(&device, path))
    }

    /// Replace an edited association atomically: validation failure preserves
    /// the old binding, and no filesystem or network operation is performed.
    pub(crate) fn replace_binding(
        &mut self,
        previous: Option<&Binding>,
        device: &str,
        path: &str,
        project: &ProjectId,
    ) -> crate::Result<()> {
        let device = DeviceKey::new(device)?;
        self.mutate(|registry| {
            if let Some(previous) = previous {
                // A concurrent edit must not silently remove another project's
                // association. The registry enforces new-path conflicts below.
                if registry.bindings_of(project).contains(previous) {
                    registry.unbind(&previous.device, &previous.path)?;
                }
            }
            registry.bind(device, path, project)?;
            Ok(())
        })
    }

    /// Remove only the association displayed by this window, not a binding
    /// another window has since reassigned to another project.
    pub(crate) fn remove_binding(&mut self, binding: &Binding) -> crate::Result<()> {
        self.mutate(|registry| {
            if registry.bindings_of(&binding.project).contains(binding) {
                registry.unbind(&binding.device, &binding.path)?;
            }
            Ok(())
        })
    }

    pub(crate) fn assign(
        &mut self,
        device: &str,
        session: &str,
        project: &ProjectId,
    ) -> crate::Result<()> {
        let session = SessionRef::new(DeviceKey::new(device)?, session);
        self.mutate(|registry| registry.assign(session, project))
    }

    /// Whether an explicit assignment was removed.
    pub(crate) fn unassign(&mut self, device: &str, session: &str) -> crate::Result<bool> {
        let session = SessionRef::new(DeviceKey::new(device)?, session);
        self.mutate(|registry| Ok(registry.unassign(&session)))
    }

    /// Drops the folder bindings and session assignments of the device with
    /// endpoint id `device`. Call it only when the device was removed for good;
    /// disabling it or failing to reach it keeps them. Whether anything went.
    pub(crate) fn forget_device(&mut self, device: &str) -> crate::Result<bool> {
        let device = DeviceKey::new(device)?;
        let mut removed = false;
        self.mutate(|registry| {
            removed = registry.forget_device(&device);
            Ok(())
        })?;
        Ok(removed)
    }

    /// Provision projects for sessions that saved workspaces can identify.
    /// Runs the batch through [`Self::mutate`], so the registry is cloned once
    /// for all sessions, the three-way save picks up other processes' edits,
    /// and a failure or a save that would exceed the limit leaves it as it was.
    /// Returns whether anything was added.
    pub(crate) fn discover<'a, I>(&mut self, sessions: I) -> crate::Result<bool>
    where
        I: IntoIterator<Item = (&'a SessionRef, &'a [SessionWorkspace])>,
    {
        self.mutate(|registry| registry.discover_sessions(sessions))
    }

    /// The project of a session on the device with endpoint id `device`.
    // The control handlers (CLI/socket) call this; the GUI itself does not.
    #[allow(dead_code)]
    pub(crate) fn resolve_session(
        &self,
        device: &str,
        session: &str,
        workspaces: &[SessionWorkspace],
    ) -> crate::Result<Resolution> {
        Ok(self.registry.resolve(
            &SessionRef::new(DeviceKey::new(device)?, session),
            workspaces,
        ))
    }
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_file_name(BACKUP)
}

fn read_registry(path: &Path) -> crate::Result<Registry> {
    let limit = Registry::MAX_JSON_BYTES as u64;
    match state_file::read(path, limit)? {
        None => Ok(Registry::new()),
        Some(bytes) => Ok(Registry::from_json(&bytes)?),
    }
}
