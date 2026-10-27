//! The grouping projection: a Window's listed sessions mapped to group keys and
//! headed groups. It is display only. The Window's member order is never
//! changed, a group lists its members in that order, and groups sit in a fixed
//! order: projects by name, then those without one; statuses by priority.
//!
//! The projection is pure. Where a session's project comes from (the saved
//! workspaces cached by `scan`, resolved against the projects registry) and how
//! its status is summed up live in `state`. A new dimension is one more
//! `Grouping` arm in [`project`] and one more key variant.

use crate::{containers::Grouping, session_mr::Bucket};
use herdr_client::ProjectId;
use std::collections::HashMap;

mod header;
mod mr;
mod mr_poll;
mod scan;
mod state;

pub(crate) use state::{Section, State};

#[cfg(test)]
mod tests;

/// What a session is doing, most urgent first: this declaration order is the
/// order of status groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Status {
    /// An agent is blocked on the user.
    Waiting,
    /// An agent is working.
    Working,
    /// Connected, with nothing running or waiting. Finished and unknown agents
    /// count as idle.
    Idle,
    /// Not connected: connecting, starting, or dropped.
    Disconnected,
    /// Switched off, or detached by the user.
    Stopped,
}

impl Status {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Waiting => "Waiting",
            Self::Working => "Working",
            Self::Idle => "Idle",
            Self::Disconnected => "Disconnected",
            Self::Stopped => "Stopped",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Disconnected => "disconnected",
            Self::Stopped => "stopped",
        }
    }
}

/// Which group a session falls in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GroupKey {
    Project(ProjectId),
    Status(Status),
    Mr(Bucket),
    /// A session no project claims.
    Unclassified,
}

impl GroupKey {
    /// A stable text form for element ids.
    pub(crate) fn slug(&self) -> String {
        match self {
            Self::Project(id) => format!("project-{id}"),
            Self::Status(status) => format!("status-{}", status.slug()),
            Self::Mr(bucket) => format!("mr-{}", bucket.slug()),
            Self::Unclassified => "unclassified".to_owned(),
        }
    }
}

/// A project as a group header names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectTag {
    pub id: ProjectId,
    pub name: String,
}

/// One listed session and what grouping needs to know about it. `item` is the
/// caller's own handle (an endpoint index) and comes back in [`Group::items`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Member {
    pub item: usize,
    pub project: Option<ProjectTag>,
    pub status: Status,
    /// The session's pull request bucket; only MR grouping reads it.
    pub mr: Bucket,
}

/// A headed run of sessions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub key: GroupKey,
    pub title: String,
    /// The members' `item`s, in the order `members` had them.
    pub items: Vec<usize>,
}

/// The groups of `members` (already in Window order) under `grouping`, or
/// `None` when the sidebar shows them flat. Only non-empty groups appear.
pub(crate) fn project(grouping: Grouping, members: &[Member]) -> Option<Vec<Group>> {
    match grouping {
        Grouping::None => None,
        Grouping::Project => Some(by_project(members)),
        Grouping::Status => Some(by_status(members)),
        Grouping::Mr => Some(mr::by_mr(members)),
    }
}

fn by_project(members: &[Member]) -> Vec<Group> {
    // Each project group with the tag that sorts it: by name, then id.
    let mut groups: Vec<(ProjectTag, Vec<usize>)> = Vec::new();
    let mut slots: HashMap<&ProjectId, usize> = HashMap::new();
    let mut unclassified = Vec::new();
    for member in members {
        match &member.project {
            Some(tag) => {
                let slot = *slots.entry(&tag.id).or_insert_with(|| {
                    groups.push((tag.clone(), Vec::new()));
                    groups.len() - 1
                });
                groups[slot].1.push(member.item);
            }
            None => unclassified.push(member.item),
        }
    }
    groups.sort_by_cached_key(|(tag, _)| (tag.name.to_lowercase(), tag.id.clone()));
    let mut groups: Vec<Group> = groups
        .into_iter()
        .map(|(tag, items)| Group {
            key: GroupKey::Project(tag.id),
            title: tag.name,
            items,
        })
        .collect();
    if !unclassified.is_empty() {
        groups.push(Group {
            key: GroupKey::Unclassified,
            title: "No project".to_owned(),
            items: unclassified,
        });
    }
    groups
}

fn by_status(members: &[Member]) -> Vec<Group> {
    let mut buckets: Vec<(Status, Vec<usize>)> = Vec::new();
    for member in members {
        match buckets
            .iter_mut()
            .find(|(status, _)| *status == member.status)
        {
            Some((_, items)) => items.push(member.item),
            None => buckets.push((member.status, vec![member.item])),
        }
    }
    buckets.sort_by_key(|(status, _)| *status);
    buckets
        .into_iter()
        .map(|(status, items)| Group {
            key: GroupKey::Status(status),
            title: status.title().to_owned(),
            items,
        })
        .collect()
}
