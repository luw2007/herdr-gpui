//! Logical Windows: the tmux-window-like sets of session workspaces one native
//! window switches between. A container only *names* sessions by device and
//! session; the connections stay with the native window's endpoints, so
//! switching, closing or removing here can never stop a Herdr session.
use herdr_client::ConnectTarget;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

type Result<T, E = ContainerError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContainerError {
    #[error("Window title must be 1-64 printable characters")]
    InvalidTitle,
    #[error("Invalid saved window containers")]
    InvalidState,
    #[error("No such window")]
    NoSuchContainer,
    #[error("At most {0} windows are allowed")]
    TooManyContainers(usize),
    #[error("At most {0} sessions fit one window")]
    TooManySlots(usize),
}

pub(crate) const MAX_CONTAINERS: usize = 32;
pub(crate) const MAX_SLOTS: usize = 64;
const MAX_TITLE: usize = 64;
const MAX_DEVICE: usize = 256;
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

/// One Herdr session: the device that runs it and its name there. This is the
/// identity of a workspace row and of everything keyed to it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct SessionRef {
    pub device: String,
    pub session: String,
}

impl SessionRef {
    pub(crate) fn new(device: impl Into<String>, session: impl Into<String>) -> Self {
        Self {
            device: device.into(),
            session: session.into(),
        }
    }

    fn valid(&self) -> bool {
        !self.device.is_empty()
            && self.device.len() <= MAX_DEVICE
            && !self.device.chars().any(char::is_control)
            && !self.session.is_empty()
            && self.session.len() <= MAX_SESSION
            && !self.session.chars().any(char::is_control)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Container {
    pub id: u64,
    pub title: String,
    pub slots: Vec<SessionRef>,
    #[serde(default)]
    pub selected: Option<SessionRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContainerSet {
    containers: Vec<Container>,
    active: usize,
    next_id: u64,
}

/// A title the strip can paint: trimmed, nonempty, bounded, no controls.
pub(crate) fn clean_title(title: &str) -> Result<String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE || title.chars().any(char::is_control)
    {
        return Err(ContainerError::InvalidTitle);
    }
    Ok(title.to_owned())
}

impl ContainerSet {
    /// One container holding `base`, the session the window was opened on.
    pub(crate) fn new(base: SessionRef) -> Self {
        Self {
            containers: vec![Container {
                id: 1,
                title: "Window 1".to_owned(),
                slots: vec![base.clone()],
                selected: Some(base),
            }],
            active: 0,
            next_id: 2,
        }
    }

    /// Checks data read from disk or the wire against every bound.
    pub(crate) fn validate(self) -> Result<Self> {
        let mut ids = HashSet::new();
        let ok = !self.containers.is_empty()
            && self.containers.len() <= MAX_CONTAINERS
            && self.active < self.containers.len()
            && self.containers.iter().all(|container| {
                let mut seen = HashSet::new();
                ids.insert(container.id)
                    && container.id < self.next_id
                    && clean_title(&container.title).is_ok_and(|t| t == container.title)
                    && container.slots.len() <= MAX_SLOTS
                    && container
                        .slots
                        .iter()
                        .all(|slot| slot.valid() && seen.insert(slot))
                    && container
                        .selected
                        .as_ref()
                        .is_none_or(|selected| container.slots.contains(selected))
            });
        if ok {
            Ok(self)
        } else {
            Err(ContainerError::InvalidState)
        }
    }

    pub(crate) fn containers(&self) -> &[Container] {
        &self.containers
    }

    pub(crate) fn active_index(&self) -> usize {
        self.active
    }

    pub(crate) fn active(&self) -> &Container {
        &self.containers[self.active]
    }

    pub(crate) fn index_of(&self, id: u64) -> Option<usize> {
        self.containers.iter().position(|c| c.id == id)
    }

    /// Adds an empty container and makes it the active one.
    pub(crate) fn create(&mut self, title: Option<&str>) -> Result<usize> {
        if self.containers.len() >= MAX_CONTAINERS {
            return Err(ContainerError::TooManyContainers(MAX_CONTAINERS));
        }
        let id = self.next_id;
        let title = match title {
            Some(title) => clean_title(title)?,
            None => format!("Window {id}"),
        };
        self.next_id += 1;
        self.containers.push(Container {
            id,
            title,
            slots: Vec::new(),
            selected: None,
        });
        self.active = self.containers.len() - 1;
        Ok(self.active)
    }

    pub(crate) fn activate(&mut self, index: usize) -> bool {
        if index >= self.containers.len() {
            return false;
        }
        self.active = index;
        true
    }

    /// Steps the active container, wrapping.
    pub(crate) fn step(&mut self, forward: bool) {
        let len = self.containers.len();
        self.active = if forward {
            (self.active + 1) % len
        } else {
            (self.active + len - 1) % len
        };
    }

    /// Drops container `index` from the view. The last one is replaced by a
    /// fresh empty container, so a native window always has one.
    pub(crate) fn close(&mut self, index: usize) -> Result<()> {
        if index >= self.containers.len() {
            return Err(ContainerError::NoSuchContainer);
        }
        self.containers.remove(index);
        if self.containers.is_empty() {
            let id = self.next_id;
            self.next_id += 1;
            self.containers.push(Container {
                id,
                title: format!("Window {id}"),
                slots: Vec::new(),
                selected: None,
            });
            self.active = 0;
        } else if index < self.active || self.active >= self.containers.len() {
            self.active = self.active.saturating_sub(1);
        }
        Ok(())
    }

    /// Attaches `session` to container `index`, selecting it there. A session
    /// already in that container is selected instead of listed twice; returns
    /// whether it was added.
    pub(crate) fn attach(&mut self, index: usize, session: SessionRef) -> Result<bool> {
        let container = self
            .containers
            .get_mut(index)
            .ok_or(ContainerError::NoSuchContainer)?;
        let added = !container.slots.contains(&session);
        if added {
            if container.slots.len() >= MAX_SLOTS {
                return Err(ContainerError::TooManySlots(MAX_SLOTS));
            }
            container.slots.push(session.clone());
        }
        container.selected = Some(session);
        Ok(added)
    }

    /// Removes `session` from container `index`; the selection moves to the
    /// neighbor that took its place. Returns whether it was there.
    pub(crate) fn detach(&mut self, index: usize, session: &SessionRef) -> bool {
        let Some(container) = self.containers.get_mut(index) else {
            return false;
        };
        let Some(position) = container.slots.iter().position(|s| s == session) else {
            return false;
        };
        container.slots.remove(position);
        if container.selected.as_ref() == Some(session) {
            container.selected = container
                .slots
                .get(position.min(container.slots.len().saturating_sub(1)))
                .cloned();
        }
        true
    }

    /// Selects `session` in the active container if it is attached there.
    pub(crate) fn select(&mut self, session: &SessionRef) -> bool {
        let container = &mut self.containers[self.active];
        if !container.slots.contains(session) {
            return false;
        }
        container.selected = Some(session.clone());
        true
    }

    /// Whether any container of this native window names `session`.
    pub(crate) fn references(&self, session: &SessionRef) -> bool {
        self.containers
            .iter()
            .any(|container| container.slots.contains(session))
    }

    /// Every distinct session any container names.
    pub(crate) fn sessions(&self) -> impl Iterator<Item = &SessionRef> {
        let mut seen = HashSet::new();
        self.containers
            .iter()
            .flat_map(|container| &container.slots)
            .filter(move |slot| seen.insert(*slot))
    }

    /// Rewrites every container's `device`/`from` slot to `to`, listing it once.
    /// A deleted session's slots move to the device's default session.
    pub(crate) fn replace_session(&mut self, device: &str, from: &str, to: &str) {
        for container in &mut self.containers {
            let rewrite = |slot: &mut SessionRef| {
                if slot.device == device && slot.session == from {
                    slot.session = to.to_owned();
                }
            };
            container.slots.iter_mut().for_each(rewrite);
            if let Some(selected) = container.selected.as_mut() {
                rewrite(selected);
            }
            let mut seen = HashSet::new();
            container.slots.retain(|slot| seen.insert(slot.clone()));
        }
    }

    /// Drops slots `keep` rejects, such as those of a device that left the catalog.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&SessionRef) -> bool) {
        for container in &mut self.containers {
            container.slots.retain(&mut keep);
            if container
                .selected
                .as_ref()
                .is_some_and(|s| !container.slots.contains(s))
            {
                container.selected = container.slots.first().cloned();
            }
        }
    }
}

#[cfg(test)]
mod tests;
