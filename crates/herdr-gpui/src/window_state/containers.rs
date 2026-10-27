//! Which Window each native window shows, saved beside the geometry. The file
//! is a list parallel to the geometry list: entry `n` is the id of the Window
//! the `n`th native window shows, `null` when that window shows none yet. The
//! Windows themselves are global (`containers::Containers`).
use super::MAX_WINDOWS;
use crate::{Error, Result, containers::ContainerId, state_file};
use gpui::WindowId;
use std::{collections::VecDeque, path::Path};

pub(super) type Writer = state_file::Writer<Vec<Option<ContainerId>>>;

const LIMIT: u64 = 64 * 1024;

#[derive(Default)]
pub(super) struct Store {
    /// Ids read at startup, handed to windows in the order they open.
    restored: VecDeque<Option<ContainerId>>,
    /// The id `placement` just popped, waiting for its window to claim it.
    pending: Option<ContainerId>,
    live: Vec<(WindowId, ContainerId)>,
    pub(super) writer: Option<Writer>,
}

/// Reads the saved ids; anything out of bounds rejects the whole file, as the
/// geometry file does.
pub(super) fn read(path: &Path) -> Result<Vec<Option<ContainerId>>> {
    let Some(bytes) = state_file::read(path, LIMIT)? else {
        return Ok(Vec::new());
    };
    let shown: Vec<Option<ContainerId>> = serde_json::from_slice(&bytes)?;
    if shown.len() > MAX_WINDOWS {
        return Err(Error::InvalidWindowState);
    }
    Ok(shown)
}

impl Store {
    pub(super) fn new(restored: Vec<Option<ContainerId>>, writer: Option<Writer>) -> Self {
        Self {
            restored: restored.into(),
            pending: None,
            live: Vec::new(),
            writer,
        }
    }

    /// Called with the geometry pop: the next window's saved Window, if any.
    pub(super) fn advance(&mut self) {
        self.pending = self.restored.pop_front().flatten();
    }

    pub(super) fn take(&mut self) -> Option<ContainerId> {
        self.pending.take()
    }

    pub(super) fn record(&mut self, id: WindowId, shown: ContainerId) -> bool {
        match self.live.iter_mut().find(|(key, _)| *key == id) {
            Some((_, old)) if *old == shown => return false,
            Some((_, old)) => *old = shown,
            None => self.live.push((id, shown)),
        }
        true
    }

    pub(super) fn forget(&mut self, id: WindowId) {
        self.live.retain(|(key, _)| *key != id);
    }

    /// The ids in the order of `windows`, the live geometry order.
    pub(super) fn snapshot(&self, windows: &[WindowId]) -> Vec<Option<ContainerId>> {
        windows
            .iter()
            .take(MAX_WINDOWS)
            .map(|id| {
                self.live
                    .iter()
                    .find(|(key, _)| key == id)
                    .map(|(_, shown)| *shown)
            })
            .collect()
    }

    pub(super) fn save(&self, windows: &[WindowId]) {
        if let Some(writer) = &self.writer {
            writer.save(self.snapshot(windows));
        }
    }
}

#[cfg(test)]
mod tests;
