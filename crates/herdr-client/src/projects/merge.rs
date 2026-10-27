//! Combining two processes' edits of one registry file, and dropping a device.
//!
//! Two app processes may each hold a registry loaded at different times. When
//! one saves after the other, [`Registry::merge`] applies this process's
//! changes (what differs from the registry it started from) on top of what is
//! on disk now, so neither side's edits are lost.
use super::{DeviceKey, ProjectError, Registry};
use std::collections::{BTreeMap, BTreeSet};

/// Three-way merge of one map: a key `mine` changed (added, edited, removed)
/// takes `mine`'s value; every other key follows `theirs`.
fn merge_map<K: Ord + Clone, V: Clone + PartialEq>(
    base: &BTreeMap<K, V>,
    mine: &BTreeMap<K, V>,
    theirs: &BTreeMap<K, V>,
) -> BTreeMap<K, V> {
    let keys: BTreeSet<&K> = base
        .keys()
        .chain(mine.keys())
        .chain(theirs.keys())
        .collect();
    let mut merged = BTreeMap::new();
    for key in keys {
        let chosen = if mine.get(key) == base.get(key) {
            theirs.get(key)
        } else {
            mine.get(key)
        };
        if let Some(value) = chosen {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

impl Registry {
    /// The registry with `mine`'s changes since `base` applied on top of
    /// `theirs`. Bindings and assignments of a project that no longer exists
    /// are dropped. Conflicting names or repositories are rejected. The result
    /// is checked like a file read back, so it never breaks a registry
    /// invariant or a size cap.
    pub fn merge(base: &Self, mine: &Self, theirs: &Self) -> Result<Self, ProjectError> {
        let mut merged = Self {
            projects: merge_map(&base.projects, &mine.projects, &theirs.projects),
            bindings: merge_map(&base.bindings, &mine.bindings, &theirs.bindings),
            assignments: merge_map(&base.assignments, &mine.assignments, &theirs.assignments),
            inferred: merge_map(&base.inferred, &mine.inferred, &theirs.inferred),
        };
        let projects = &merged.projects;
        merged.bindings.retain(|_, id| projects.contains_key(id));
        merged.assignments.retain(|_, id| projects.contains_key(id));
        merged.inferred.retain(|_, id| projects.contains_key(id));
        Self::from_json(&merged.to_json()?)?;
        Ok(merged)
    }

    /// Drop every folder binding, inferred path, and session assignment of
    /// `device`, for a device that was removed for good. Projects stay.
    /// Whether anything was removed.
    pub fn forget_device(&mut self, device: &DeviceKey) -> bool {
        let before = self.bindings.len() + self.inferred.len() + self.assignments.len();
        self.bindings.retain(|(bound, _), _| bound != device);
        self.inferred.retain(|(bound, _), _| bound != device);
        self.assignments
            .retain(|session, _| &session.device != device);
        self.bindings.len() + self.inferred.len() + self.assignments.len() != before
    }
}
