//! Windows: the tmux-window-like, ordered sets of sessions the app lists.
//!
//! A Window is global: one registry, shared by every native window, with a
//! stable id, a unique name, ordered members and a selected member. A member
//! is a [`SessionRef`]: only a (device, session) pair names a session, so the
//! same name on two devices never mixes. A Window only *names* sessions; the
//! connections stay with each native window's endpoints, so switching,
//! closing, attaching or detaching here can never stop a Herdr session.
//!
//! The same Window may be open in several native windows at once. Each native
//! window keeps its own endpoints and remembers only *which* Window it shows
//! (`window_state`); membership edits go through the shared registry, bump
//! its revision, and every other native window follows on its next tick.
//!
//! Aggregation keys (project, status, merge request) are derived from the
//! member sessions when needed and are never stored here; [`Grouping`] only
//! records which key a Window should aggregate by.
use herdr_client::{ConnectTarget, DeviceKey, SessionRef};
use std::collections::HashSet;
use uuid::Uuid;

mod file;
mod legacy;
mod merge;
mod store;
mod view;

pub(crate) use store::Containers;
pub(crate) use view::ContainerView;
#[cfg(test)]
mod tests;

type Result<T, E = ContainerError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContainerError {
    #[error("Space name must be 1-64 printable characters")]
    InvalidTitle,
    #[error("A Space named {0:?} already exists")]
    DuplicateTitle(String),
    #[error("Invalid saved Spaces")]
    InvalidState,
    #[error("No such Space")]
    NoSuchContainer,
    #[error("The Space still lists sessions. Detach its sessions first")]
    NotEmpty,
    #[error("The session is not in that Space")]
    NoSuchMember,
    #[error("At most {0} Spaces are allowed")]
    TooManyContainers(usize),
    #[error("At most {0} sessions fit one Space")]
    TooManySlots(usize),
}

pub(crate) const MAX_CONTAINERS: usize = 32;
pub(crate) const MAX_SLOTS: usize = 64;
/// Stopped sessions remembered across restarts; the oldest are forgotten.
pub(crate) const MAX_PAUSED: usize = 256;
const MAX_TITLE: usize = 64;
const MAX_SESSION: usize = 1024;

/// The session a target attaches to. Asking the host what else it runs would
/// be an SSH round trip; an explicit socket names itself by its path.
pub(crate) fn target_session(target: &ConnectTarget) -> String {
    match target {
        ConnectTarget::Ssh { session, .. }
        | ConnectTarget::Wsl { session, .. }
        | ConnectTarget::Session { name: session, .. } => session.clone(),
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud { session, .. } => session.clone(),
        ConnectTarget::Socket(path) => path.display().to_string(),
        ConnectTarget::Local => "default".to_owned(),
    }
}

/// The session of the device with endpoint id `device`. An id the catalog
/// produced is always a valid key; anything else is cleaned rather than lost.
pub(crate) fn session_ref(device: &str, session: impl Into<String>) -> SessionRef {
    SessionRef::new(
        DeviceKey::new(device).unwrap_or_else(|_| DeviceKey::sanitized(device)),
        session,
    )
}

/// Whether `session` names something a Window may list.
pub(crate) fn valid_session(session: &str) -> bool {
    !session.is_empty() && session.len() <= MAX_SESSION && !session.chars().any(char::is_control)
}

/// A Window's stable identity: never reused, never renumbered.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub(crate) struct ContainerId(Uuid);

impl ContainerId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[cfg(any(unix, test))]
    pub(crate) fn parse(text: &str) -> Option<Self> {
        Uuid::parse_str(text).ok().map(Self)
    }
}

impl std::fmt::Display for ContainerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.as_hyphenated().fmt(f)
    }
}

/// How a Window's members are to be aggregated. Only stored this wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Grouping {
    #[default]
    None,
    Project,
    Status,
    Mr,
}

impl Grouping {
    #[cfg(unix)]
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "none" => Some(Self::None),
            "project" => Some(Self::Project),
            "status" => Some(Self::Status),
            "mr" => Some(Self::Mr),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Project => "project",
            Self::Status => "status",
            Self::Mr => "mr",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Container {
    pub id: ContainerId,
    pub title: String,
    pub members: Vec<SessionRef>,
    pub selected: Option<SessionRef>,
    pub grouping: Grouping,
}

/// A name the strip can paint: trimmed, nonempty, bounded, no controls.
pub(crate) fn clean_title(title: &str) -> Result<String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE || title.chars().any(char::is_control)
    {
        return Err(ContainerError::InvalidTitle);
    }
    Ok(title.to_owned())
}

/// The ordered Windows. It is never empty once [`ContainerSet::ensure_default`]
/// ran, which every owner does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ContainerSet {
    containers: Vec<Container>,
    /// Sessions the user stopped and has not chosen again. Every native window
    /// parks their connections instead of dialling them, across restarts, so
    /// a stopped session is not started again by merely being listed.
    paused: Vec<SessionRef>,
}

impl ContainerSet {
    /// Checks data read from disk against every bound.
    pub(crate) fn validated(containers: Vec<Container>) -> Result<Self> {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        let ok = containers.len() <= MAX_CONTAINERS
            && containers.iter().all(|container| {
                let mut seen = HashSet::new();
                ids.insert(container.id)
                    && names.insert(container.title.clone())
                    && clean_title(&container.title).is_ok_and(|t| t == container.title)
                    && container.members.len() <= MAX_SLOTS
                    && container
                        .members
                        .iter()
                        .all(|member| valid_session(&member.session) && seen.insert(member))
                    && container
                        .selected
                        .as_ref()
                        .is_none_or(|selected| container.members.contains(selected))
            });
        if ok {
            Ok(Self {
                containers,
                paused: Vec::new(),
            })
        } else {
            Err(ContainerError::InvalidState)
        }
    }

    /// The first unused `Space N`; existing saved titles are never rewritten.
    fn fresh_title(&self) -> String {
        (1..)
            .map(|n| format!("Space {n}"))
            .find(|title| !self.containers.iter().any(|c| &c.title == title))
            .unwrap_or_default()
    }

    fn push_empty(&mut self, title: String) -> ContainerId {
        let id = ContainerId::new();
        self.containers.push(Container {
            id,
            title,
            members: Vec::new(),
            selected: None,
            grouping: Grouping::None,
        });
        id
    }

    /// Guarantees at least one Window; a new one lists `base` when given.
    pub(crate) fn ensure_default(&mut self, base: Option<SessionRef>) {
        if !self.containers.is_empty() {
            return;
        }
        let title = self.fresh_title();
        let id = self.push_empty(title);
        if let Some(base) = base
            && let Some(container) = self.get_mut(id)
        {
            container.members.push(base.clone());
            container.selected = Some(base);
        }
    }

    pub(crate) fn containers(&self) -> &[Container] {
        &self.containers
    }

    pub(crate) fn get(&self, id: ContainerId) -> Option<&Container> {
        self.containers.iter().find(|c| c.id == id)
    }

    fn get_mut(&mut self, id: ContainerId) -> Option<&mut Container> {
        self.containers.iter_mut().find(|c| c.id == id)
    }

    fn entry(&mut self, id: ContainerId) -> Result<&mut Container> {
        self.get_mut(id).ok_or(ContainerError::NoSuchContainer)
    }

    pub(crate) fn index_of(&self, id: ContainerId) -> Option<usize> {
        self.containers.iter().position(|c| c.id == id)
    }

    #[cfg(any(unix, test))]
    /// The Window named `name`; names are unique.
    pub(crate) fn by_name(&self, name: &str) -> Option<ContainerId> {
        self.containers
            .iter()
            .find(|c| c.title == name)
            .map(|c| c.id)
    }

    /// Adds an empty Window at the end.
    pub(crate) fn create(&mut self, title: Option<&str>) -> Result<ContainerId> {
        if self.containers.len() >= MAX_CONTAINERS {
            return Err(ContainerError::TooManyContainers(MAX_CONTAINERS));
        }
        let title = match title {
            Some(title) => self.unique(clean_title(title)?, None)?,
            None => self.fresh_title(),
        };
        Ok(self.push_empty(title))
    }

    fn unique(&self, title: String, except: Option<ContainerId>) -> Result<String> {
        if self
            .containers
            .iter()
            .any(|c| c.title == title && Some(c.id) != except)
        {
            return Err(ContainerError::DuplicateTitle(title));
        }
        Ok(title)
    }

    pub(crate) fn rename(&mut self, id: ContainerId, title: &str) -> Result<()> {
        let title = self.unique(clean_title(title)?, Some(id))?;
        self.entry(id)?.title = title;
        Ok(())
    }

    /// Drops an empty Window. Closing the last one leaves a fresh empty
    /// default. A Window that lists sessions is refused here, in the registry
    /// write itself, so no caller can act on a stale view of its members.
    pub(crate) fn close(&mut self, id: ContainerId) -> Result<()> {
        let index = self.index_of(id).ok_or(ContainerError::NoSuchContainer)?;
        if !self.containers[index].members.is_empty() {
            return Err(ContainerError::NotEmpty);
        }
        self.containers.remove(index);
        self.ensure_default(None);
        Ok(())
    }

    #[cfg(any(unix, test))]
    /// Moves Window `id` to position `to` (clamped).
    pub(crate) fn move_container(&mut self, id: ContainerId, to: usize) -> Result<()> {
        let index = self.index_of(id).ok_or(ContainerError::NoSuchContainer)?;
        let container = self.containers.remove(index);
        self.containers
            .insert(to.min(self.containers.len()), container);
        Ok(())
    }

    pub(crate) fn set_grouping(&mut self, id: ContainerId, grouping: Grouping) -> Result<()> {
        self.entry(id)?.grouping = grouping;
        Ok(())
    }

    /// Attaches `session` to Window `id`, selecting it there. A member already
    /// there is selected instead of listed twice; returns whether it was added.
    pub(crate) fn attach(&mut self, id: ContainerId, session: SessionRef) -> Result<bool> {
        if !valid_session(&session.session) {
            return Err(ContainerError::InvalidState);
        }
        let container = self.entry(id)?;
        let added = !container.members.contains(&session);
        if added {
            if container.members.len() >= MAX_SLOTS {
                return Err(ContainerError::TooManySlots(MAX_SLOTS));
            }
            container.members.push(session.clone());
        }
        container.selected = Some(session);
        Ok(added)
    }

    /// Removes `session` from Window `id`; the selection moves to the
    /// neighbor that took its place. Returns whether it was there.
    pub(crate) fn detach(&mut self, id: ContainerId, session: &SessionRef) -> bool {
        let Some(container) = self.get_mut(id) else {
            return false;
        };
        let Some(position) = container.members.iter().position(|s| s == session) else {
            return false;
        };
        container.members.remove(position);
        if container.selected.as_ref() == Some(session) {
            container.selected = container
                .members
                .get(position.min(container.members.len().saturating_sub(1)))
                .cloned();
        }
        true
    }

    /// Selects `session` in Window `id`; whether it is a member.
    pub(crate) fn select(&mut self, id: ContainerId, session: &SessionRef) -> bool {
        match self.get_mut(id) {
            Some(container) if container.members.contains(session) => {
                container.selected = Some(session.clone());
                true
            }
            _ => false,
        }
    }

    #[cfg(any(unix, test))]
    /// Moves a member of Window `id` to position `to` (clamped).
    pub(crate) fn move_member(
        &mut self,
        id: ContainerId,
        session: &SessionRef,
        to: usize,
    ) -> Result<()> {
        let container = self.entry(id)?;
        let index = container
            .members
            .iter()
            .position(|s| s == session)
            .ok_or(ContainerError::NoSuchMember)?;
        let member = container.members.remove(index);
        container
            .members
            .insert(to.min(container.members.len()), member);
        Ok(())
    }

    /// Whether the user stopped `session` and has not chosen it since.
    pub(crate) fn is_paused(&self, session: &SessionRef) -> bool {
        self.paused.contains(session)
    }

    /// Records that `session` must not be dialled until chosen again.
    pub(crate) fn pause(&mut self, session: SessionRef) {
        if self.paused.contains(&session) {
            return;
        }
        if self.paused.len() >= MAX_PAUSED {
            self.paused.remove(0);
        }
        self.paused.push(session);
    }

    /// Lets `session` connect again; whether it was paused.
    pub(crate) fn resume(&mut self, session: &SessionRef) -> bool {
        let before = self.paused.len();
        self.paused.retain(|paused| paused != session);
        self.paused.len() != before
    }

    pub(crate) fn paused(&self) -> &[SessionRef] {
        &self.paused
    }

    /// Adopts paused sessions read from disk: invalid or surplus ones are
    /// dropped, never the file.
    pub(crate) fn with_paused(mut self, paused: impl IntoIterator<Item = SessionRef>) -> Self {
        for session in paused {
            if self.paused.len() >= MAX_PAUSED {
                break;
            }
            if valid_session(&session.session) {
                self.pause(session);
            }
        }
        self
    }

    /// Whether any Window names `session`.
    pub(crate) fn references(&self, session: &SessionRef) -> bool {
        self.containers.iter().any(|c| c.members.contains(session))
    }

    /// Every distinct session any Window names.
    pub(crate) fn sessions(&self) -> impl Iterator<Item = &SessionRef> {
        let mut seen = HashSet::new();
        self.containers
            .iter()
            .flat_map(|container| &container.members)
            .filter(move |member| seen.insert(*member))
    }

    /// Rewrites every Window's `device`/`from` member to `to`, listing it once.
    /// A deleted session's members move to the device's default session.
    pub(crate) fn replace_session(&mut self, device: &DeviceKey, from: &str, to: &str) {
        // A deleted session no longer exists to be paused; its default
        // replacement keeps whatever intent it already had.
        self.paused
            .retain(|paused| !(paused.device == *device && paused.session == from));
        for container in &mut self.containers {
            let rewrite = |member: &mut SessionRef| {
                if member.device == *device && member.session == from {
                    member.session = to.to_owned();
                }
            };
            container.members.iter_mut().for_each(rewrite);
            if let Some(selected) = container.selected.as_mut() {
                rewrite(selected);
            }
            let mut seen = HashSet::new();
            container
                .members
                .retain(|member| seen.insert(member.clone()));
        }
    }

    /// Drops members `keep` rejects, such as those of a device that left the
    /// catalog.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&SessionRef) -> bool) {
        self.paused.retain(&mut keep);
        for container in &mut self.containers {
            container.members.retain(&mut keep);
            if container
                .selected
                .as_ref()
                .is_some_and(|s| !container.members.contains(s))
            {
                container.selected = container.members.first().cloned();
            }
        }
    }
}
