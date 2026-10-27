//! Saving a registry that several app processes share (`windows.json`,
//! `projects.json`).
//!
//! Each process holds the registry it loaded plus its own edits. A plain
//! overwrite would drop what another process saved in between, so the writer
//! worker takes the cross-process lock ([`state_file::lock`]), rereads the
//! file, and merges this process's changes since its last save (three-way,
//! `base` being the last state it wrote or loaded) onto what is on disk now.
//! The worker never blocks the UI thread. When the merge brought in another
//! process's edits, the result is parked in an [`Inbox`]; the store adopts it
//! on the UI thread, merging again over any edit made since the save began.
use crate::state_file;
#[cfg(test)]
mod tests;
use serde::{Serialize, Serializer};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

/// A registry that can be merged.
pub(crate) trait Shared: Clone + PartialEq + Send + 'static {
    /// The most bytes the file may hold.
    const LIMIT: u64;
    fn decode(bytes: &[u8]) -> crate::Result<Self>;
    fn encode(&self) -> crate::Result<Vec<u8>>;
    /// `mine`'s changes since `base` applied on top of `theirs`.
    fn merge(base: &Self, mine: &Self, theirs: &Self) -> crate::Result<Self>;
}

/// What the writer saves: a copy of the registry, encoded on the worker.
pub(crate) struct Snapshot<S>(pub(crate) S);

impl<S: Shared> Serialize for Snapshot<S> {
    fn serialize<T: Serializer>(&self, serializer: T) -> Result<T::Ok, T::Error> {
        let bytes = self.0.encode().map_err(serde::ser::Error::custom)?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(serde::ser::Error::custom)?;
        value.serialize(serializer)
    }
}

/// The newest save that picked up another process's edits: the snapshot that
/// was saved and the registry that resulted.
pub(crate) struct Inbox<S>(Mutex<Option<(S, S)>>);

impl<S> Default for Inbox<S> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl<S> Inbox<S> {
    fn publish(&self, saved: S, merged: S) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some((saved, merged));
        }
    }

    /// Whether something waits. Never blocks: a busy inbox counts as empty
    /// and is looked at again on the next tick.
    pub(crate) fn has_news(&self) -> bool {
        self.0.try_lock().is_ok_and(|slot| slot.is_some())
    }

    pub(crate) fn take(&self) -> Option<(S, S)> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// The worker's save step for a store that started from `base`. An unreadable
/// file found at save time is renamed to `backup` (when given) and replaced.
pub(crate) fn saver<S: Shared>(
    base: S,
    backup: Option<PathBuf>,
    inbox: std::sync::Arc<Inbox<S>>,
) -> impl FnMut(&Path, Snapshot<S>) -> crate::Result<()> + Send + 'static {
    let mut base = base;
    move |path, Snapshot(mine)| {
        let _lock = state_file::lock(path)?;
        let theirs = match state_file::read(path, S::LIMIT)
            .and_then(|bytes| bytes.map(|bytes| S::decode(&bytes)).transpose())
        {
            Ok(theirs) => theirs,
            Err(error) => {
                tracing::warn!(%error, "Cannot read the shared state file; replacing it");
                if let Some(backup) = &backup {
                    std::fs::rename(path, backup)?;
                }
                None
            }
        };
        let merged = match &theirs {
            Some(theirs) => match S::merge(&base, &mine, theirs) {
                Ok(merged) => merged,
                Err(error) => {
                    // Cannot combine: the file wins and this process adopts
                    // it, so the UI never keeps an edit that was not saved.
                    base = mine.clone();
                    inbox.publish(mine, theirs.clone());
                    return Err(error);
                }
            },
            None => mine.clone(),
        };
        if theirs.as_ref() != Some(&merged) {
            state_file::write(path, &Snapshot(merged.clone()))?;
        }
        // Baseline follows submitted local snapshots, not the disk merge:
        // a later queued snapshot may not have adopted imported changes yet.
        base = mine.clone();
        if merged != mine {
            inbox.publish(mine, merged);
        }
        Ok(())
    }
}

/// Adopts a parked save into the store's `current` registry: whatever was
/// edited since the save began is kept on top of what the merge brought in.
/// `None` when nothing changed. The second value says whether `current` still
/// differs from what is on disk, so the caller must save again.
pub(crate) fn adopt<S: Shared>(inbox: &Inbox<S>, current: &S) -> crate::Result<Option<(S, bool)>> {
    let Some((saved, merged)) = inbox.take() else {
        return Ok(None);
    };
    let next = match S::merge(&saved, current, &merged) {
        Ok(next) => next,
        Err(error) => {
            // A newer edit can conflict too. Adopt the valid saved state
            // rather than consuming the only reconciliation and leaving an
            // unsaveable local registry in place.
            tracing::warn!(%error, "Cannot adopt concurrent edits; keeping the saved file");
            merged.clone()
        }
    };
    if &next == current {
        return Ok(None);
    }
    let dirty = next != merged;
    Ok(Some((next, dirty)))
}
