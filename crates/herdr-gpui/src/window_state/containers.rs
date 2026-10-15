//! The logical Windows of each native window, saved beside the geometry.
//! The file is a list parallel to the geometry list: entry `n` belongs to the
//! `n`th native window, `null` when that window has none to restore.
use super::MAX_WINDOWS;
use crate::{
    Error, Result,
    containers::{ContainerError, ContainerSet},
    state_file,
};
use gpui::WindowId;
use std::{collections::VecDeque, path::Path};

pub(super) type Writer = state_file::Writer<Vec<Option<ContainerSet>>>;

const LIMIT: u64 = 1024 * 1024;

#[derive(Default)]
pub(super) struct Store {
    /// Sets read at startup, handed to windows in the order they open.
    restored: VecDeque<Option<ContainerSet>>,
    /// The set `placement` just popped, waiting for its window to claim it.
    pending: Option<ContainerSet>,
    live: Vec<(WindowId, ContainerSet)>,
    pub(super) writer: Option<Writer>,
}

/// Reads and validates the saved sets; anything out of bounds rejects the
/// whole file, as the geometry file does.
pub(super) fn read(path: &Path) -> Result<Vec<Option<ContainerSet>>> {
    let Some(bytes) = state_file::read(path, LIMIT)? else {
        return Ok(Vec::new());
    };
    let sets: Vec<Option<ContainerSet>> = serde_json::from_slice(&bytes)?;
    if sets.len() > MAX_WINDOWS {
        return Err(Error::Container(ContainerError::InvalidState));
    }
    sets.into_iter()
        .map(|set| set.map(ContainerSet::validate).transpose())
        .collect::<std::result::Result<_, _>>()
        .map_err(Error::Container)
}

impl Store {
    pub(super) fn new(restored: Vec<Option<ContainerSet>>, writer: Option<Writer>) -> Self {
        Self {
            restored: restored.into(),
            pending: None,
            live: Vec::new(),
            writer,
        }
    }

    /// Called with the geometry pop: the next window's saved set, if any.
    pub(super) fn advance(&mut self) {
        self.pending = self.restored.pop_front().flatten();
    }

    pub(super) fn take(&mut self) -> Option<ContainerSet> {
        self.pending.take()
    }

    pub(super) fn record(&mut self, id: WindowId, set: &ContainerSet) -> bool {
        match self.live.iter_mut().find(|(key, _)| *key == id) {
            Some((_, old)) if old == set => return false,
            Some((_, old)) => *old = set.clone(),
            None => self.live.push((id, set.clone())),
        }
        true
    }

    pub(super) fn forget(&mut self, id: WindowId) {
        self.live.retain(|(key, _)| *key != id);
    }

    /// The sets in the order of `windows`, the live geometry order.
    pub(super) fn snapshot(&self, windows: &[WindowId]) -> Vec<Option<ContainerSet>> {
        windows
            .iter()
            .take(MAX_WINDOWS)
            .map(|id| {
                self.live
                    .iter()
                    .find(|(key, _)| key == id)
                    .map(|(_, set)| set.clone())
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
