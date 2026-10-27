//! The app's Windows, shared by every native window like the browser tabs and
//! the projects. The registry lives in `windows.json` in the state directory
//! and is GUI-local; no daemon or protocol carries it.
//!
//! Loading never fails the app. A missing file is an empty registry. A file
//! that is corrupt, oversized or of an unsupported version also starts empty,
//! with a warning, and is left untouched until the user changes something; the
//! first save then renames it to `windows.json.bad` (replacing an older
//! backup) before writing, so a bad file is never silently destroyed.
//!
//! The retired per-native-window `window-containers.json` is migrated once:
//! when `windows.json` does not exist and the old file does, its Windows
//! become global Windows (same-named ones get a number), `windows.json` is
//! written, and the old file is renamed `window-containers.json.migrated`.
//!
//! Saving hands a snapshot to a [`state_file::Writer`] worker: the UI thread
//! never waits on the disk. Every mutation bumps [`Containers::revision`] and
//! notifies the global's observers, which is how every native window follows.
use super::{
    ContainerError, ContainerId, ContainerSet,
    file::{self, MAX_JSON_BYTES},
    legacy,
};
use crate::{
    shared_state::{self, Shared},
    state_file,
};
use gpui::{App, BorrowAppContext, Global};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

const FILE: &str = "windows.json";
const BACKUP: &str = "windows.json.bad";
const LEGACY: &str = "window-containers.json";
const MIGRATED: &str = "window-containers.json.migrated";

impl Shared for ContainerSet {
    const LIMIT: u64 = MAX_JSON_BYTES as u64;
    fn decode(bytes: &[u8]) -> crate::Result<Self> {
        Ok(file::decode(bytes)?)
    }
    fn encode(&self) -> crate::Result<Vec<u8>> {
        file::encode(self)
    }
    fn merge(base: &Self, mine: &Self, theirs: &Self) -> crate::Result<Self> {
        Ok(Self::merge(base, mine, theirs)?)
    }
}
type Snapshot = shared_state::Snapshot<ContainerSet>;

#[derive(Default)]
pub(crate) struct Containers {
    set: ContainerSet,
    revision: u64,
    writer: Option<state_file::Writer<Snapshot>>,
    quitting: bool,
    /// Saves that picked up another process's edits, waiting to be adopted.
    inbox: Arc<shared_state::Inbox<ContainerSet>>,
    /// The Window each native window showed, when this launch migrated the
    /// retired per-window file. Taken once by the window state.
    migrated_shown: Option<Vec<Option<ContainerId>>>,
}

impl Global for Containers {}

impl Containers {
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
        let mut migrated_shown = None;
        let set = match read_set(&path) {
            Ok(Some(set)) => set,
            Ok(None) => match migrate(&path) {
                Some(migrated) => {
                    migrated_shown = Some(migrated.shown);
                    migrated.set
                }
                None => ContainerSet::default(),
            },
            Err(error) => {
                tracing::warn!(%error, "Cannot restore windows; starting empty and keeping the file");
                ContainerSet::default()
            }
        };
        let backup = Some(path.with_file_name(BACKUP));
        let inbox = Arc::new(shared_state::Inbox::default());
        let save = shared_state::saver(set.clone(), backup.clone(), inbox.clone());
        let writer = match state_file::Writer::start_with("windows", path, None, save) {
            Ok(writer) => Some(writer),
            Err(error) => {
                tracing::warn!(%error, "Cannot start the windows worker");
                None
            }
        };
        Self {
            set,
            revision: 0,
            writer,
            quitting: false,
            inbox,
            migrated_shown,
        }
    }

    pub(crate) fn take_migrated_shown(&mut self) -> Option<Vec<Option<ContainerId>>> {
        self.migrated_shown.take()
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

    /// A read-only view, or an empty set when none was installed.
    pub(crate) fn read<R>(cx: &App, f: impl FnOnce(&ContainerSet) -> R) -> R {
        match cx.try_global::<Self>() {
            Some(store) => f(&store.set),
            None => f(&ContainerSet::default()),
        }
    }

    /// Increases by one on every successful mutation, so a view can tell its
    /// copy is stale without comparing sets.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether a save merged in another process's edits that this store has
    /// not adopted yet. Cheap; the window ticks call it.
    pub(crate) fn has_external_edits(&self) -> bool {
        self.inbox.has_news()
    }
    /// Adopts what another process saved, as merged by this process's last
    /// save, keeping any edit made since. Counts the revision when the set
    /// changed.
    pub(crate) fn adopt_external(&mut self) {
        match shared_state::adopt(&self.inbox, &self.set) {
            Ok(Some((next, dirty))) => {
                self.set = next;
                self.revision += 1;
                if dirty {
                    self.save();
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "Cannot merge another process's windows"),
        }
    }
    pub(crate) fn set(&self) -> &ContainerSet {
        &self.set
    }

    /// Applies `change`; on success of an actual change counts the revision
    /// and queues a save. A failed change leaves the set as it was.
    pub(crate) fn mutate<R>(
        &mut self,
        change: impl FnOnce(&mut ContainerSet) -> Result<R, ContainerError>,
    ) -> Result<R, ContainerError> {
        let mut next = self.set.clone();
        let result = change(&mut next)?;
        if next != self.set {
            file::encode(&next).map_err(|_| ContainerError::InvalidState)?;
            self.set = next;
            self.revision += 1;
            self.save();
        }
        Ok(result)
    }

    /// Writes what is pending and stops the worker.
    #[cfg(test)]
    pub(crate) fn finish(mut self) {
        if let Some(writer) = self.writer.take() {
            writer.finish();
        }
    }

    fn save(&self) {
        if self.quitting {
            return;
        }
        if let Some(writer) = &self.writer {
            writer.save(shared_state::Snapshot(self.set.clone()));
        }
    }
}

/// `Ok(None)` when the file does not exist.
fn read_set(path: &Path) -> crate::Result<Option<ContainerSet>> {
    match state_file::read(path, MAX_JSON_BYTES as u64)? {
        None => Ok(None),
        Some(bytes) => Ok(Some(file::decode(&bytes)?)),
    }
}

/// Seeds the registry from the retired per-native-window file, if any, and
/// retires that file. A file that cannot be read starts the registry empty and
/// stays where it is.
fn migrate(path: &Path) -> Option<legacy::Migrated> {
    // Startup waits before GPUI runs; locking and rereading still belong to
    // a background writer, never the application thread.
    let path = path.to_owned();
    std::thread::Builder::new()
        .name("windows-migration".into())
        .spawn(move || migrate_on_worker(&path))
        .ok()?
        .join()
        .ok()?
}

fn migrate_on_worker(path: &Path) -> Option<legacy::Migrated> {
    let _lock = state_file::lock(path).ok()?;
    if path.exists() {
        return read_set(path).ok().flatten().map(|set| legacy::Migrated {
            set,
            shown: Vec::new(),
        });
    }
    let old = path.with_file_name(LEGACY);
    let bytes = match state_file::read(&old, legacy::MAX_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!(%error, "Cannot read the retired logical-window file");
            return None;
        }
    };
    let migrated = match legacy::migrate(&bytes) {
        Ok(migrated) => migrated,
        Err(error) => {
            tracing::warn!(%error, "Cannot migrate the retired logical-window file");
            return None;
        }
    };
    if let Err(error) = state_file::write(path, &shared_state::Snapshot(migrated.set.clone())) {
        tracing::warn!(%error, "Cannot write migrated windows");
        return None;
    }
    if let Err(error) = retire(&old) {
        tracing::warn!(%error, "Cannot rename the retired logical-window file");
    }
    Some(migrated)
}

fn retire(old: &Path) -> io::Result<()> {
    fs::rename(old, old.with_file_name(MIGRATED))
}
