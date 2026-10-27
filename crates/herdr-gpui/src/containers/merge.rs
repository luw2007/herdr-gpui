//! Combining two processes' edits of `windows.json`: `mine`'s changes since
//! `base` applied on top of what another process saved (`theirs`).
//!
//! Windows are matched by id. A Window only one side touched is taken from that
//! side; one both touched is merged field by field (title, grouping and
//! selection follow `mine` when it changed them, members are `theirs` minus
//! what `mine` removed plus what `mine` added). Window order follows `mine`
//! when it reordered, else `theirs`. Titles that collide get a number, and
//! every bound of a valid file is re-applied.
use super::{
    Container, ContainerError, ContainerId, ContainerSet, MAX_CONTAINERS, MAX_PAUSED, MAX_SLOTS,
    Result, legacy,
};
use herdr_client::SessionRef;
use std::collections::{HashMap, HashSet};

fn ids(set: &ContainerSet) -> Vec<ContainerId> {
    set.containers.iter().map(|c| c.id).collect()
}

fn by_id(set: &ContainerSet) -> HashMap<ContainerId, &Container> {
    set.containers.iter().map(|c| (c.id, c)).collect()
}

/// `theirs` minus what `mine` removed from `base`, plus what `mine` added.
fn merge_list(base: &[SessionRef], mine: &[SessionRef], theirs: &[SessionRef]) -> Vec<SessionRef> {
    let (base_set, mine_set): (HashSet<_>, HashSet<_>) =
        (base.iter().collect(), mine.iter().collect());
    let mut merged: Vec<SessionRef> = theirs
        .iter()
        .filter(|s| mine_set.contains(s) || !base_set.contains(s))
        .cloned()
        .collect();
    // Keep an explicit local reorder, while retaining additions that only
    // appeared on disk. Removals alone must not undo a remote reorder.
    let common_base: Vec<_> = base.iter().filter(|s| mine_set.contains(s)).collect();
    let common_mine: Vec<_> = mine.iter().filter(|s| base_set.contains(s)).collect();
    if common_base != common_mine {
        let retained: HashSet<_> = merged.iter().collect();
        let mut ordered: Vec<_> = mine
            .iter()
            .filter(|s| retained.contains(s))
            .cloned()
            .collect();
        ordered.extend(merged.iter().filter(|s| !mine_set.contains(s)).cloned());
        merged = ordered;
    }
    let have: HashSet<SessionRef> = merged.iter().cloned().collect();
    merged.extend(
        mine.iter()
            .filter(|s| !base_set.contains(s) && !have.contains(s))
            .cloned(),
    );
    merged
}

fn pick<T>(changed: bool, mine: T, theirs: T) -> T {
    if changed { mine } else { theirs }
}

fn merge_container(base: &Container, mine: &Container, theirs: &Container) -> Result<Container> {
    let mut members = merge_list(&base.members, &mine.members, &theirs.members);
    let mut seen = HashSet::new();
    members.retain(|member| seen.insert(member.clone()));
    if members.len() > MAX_SLOTS {
        return Err(ContainerError::TooManySlots(MAX_SLOTS));
    }
    let selected = pick(
        mine.selected != base.selected,
        mine.selected.clone(),
        theirs.selected.clone(),
    )
    .filter(|selected| members.contains(selected))
    .or_else(|| members.first().cloned());
    Ok(Container {
        id: theirs.id,
        title: pick(
            mine.title != base.title,
            mine.title.clone(),
            theirs.title.clone(),
        ),
        members,
        selected,
        grouping: pick(
            mine.grouping != base.grouping,
            mine.grouping,
            theirs.grouping,
        ),
    })
}

impl ContainerSet {
    /// `mine`'s changes since `base` on top of `theirs`.
    pub(crate) fn merge(base: &Self, mine: &Self, theirs: &Self) -> Result<Self> {
        let (base_by, mine_by, theirs_by) = (by_id(base), by_id(mine), by_id(theirs));
        let reordered = ids(mine) != ids(base);
        let order: Vec<ContainerId> = if reordered {
            let mut order = ids(mine);
            order.extend(
                ids(theirs)
                    .into_iter()
                    .filter(|id| !mine_by.contains_key(id)),
            );
            order
        } else {
            let mut order = ids(theirs);
            order.extend(
                ids(mine)
                    .into_iter()
                    .filter(|id| !theirs_by.contains_key(id)),
            );
            order
        };
        let mut containers = Vec::new();
        for id in order {
            let merged = match (base_by.get(&id), mine_by.get(&id), theirs_by.get(&id)) {
                // Closing wins over metadata-only edits, but never discards
                // members concurrently attached to an empty Window.
                (Some(base), None, Some(theirs))
                    if !theirs.members.is_empty() && theirs.members != base.members =>
                {
                    Some((*theirs).clone())
                }
                (Some(_), None, _) => None,
                (Some(base), Some(mine), None)
                    if !mine.members.is_empty() && mine.members != base.members =>
                {
                    Some((*mine).clone())
                }
                (Some(_), Some(_), None) => None,
                (None, Some(mine), None) => Some((*mine).clone()),
                (None, None, Some(theirs)) => Some((*theirs).clone()),
                (Some(base), Some(mine), Some(theirs)) => {
                    Some(merge_container(base, mine, theirs)?)
                }
                (None, Some(mine), Some(_)) => Some((*mine).clone()),
                (_, None, None) => None,
            };
            containers.extend(merged);
        }
        if containers.len() > MAX_CONTAINERS {
            return Err(ContainerError::TooManyContainers(MAX_CONTAINERS));
        }
        let mut taken = HashSet::new();
        for container in &mut containers {
            container.title = legacy::unique(&container.title, &taken);
            taken.insert(container.title.clone());
        }
        let mut merged = Self {
            containers,
            paused: Vec::new(),
        };
        let mut paused = merge_list(&base.paused, &mine.paused, &theirs.paused);
        paused.retain(|session| super::valid_session(&session.session));
        let skip = paused.len().saturating_sub(MAX_PAUSED);
        merged = merged.with_paused(paused.into_iter().skip(skip));
        merged.ensure_default(None);
        super::file::encode(&merged).map_err(|_| ContainerError::InvalidState)?;
        Ok(merged)
    }
}
