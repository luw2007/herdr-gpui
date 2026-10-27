//! The `windows.json` format: a version, then the Windows in order. Encoding
//! is deterministic (fixed field order, no maps), so equal registries write
//! equal bytes.
use super::{
    Container, ContainerError, ContainerId, ContainerSet, Grouping, MAX_CONTAINERS, MAX_SLOTS,
    valid_session,
};
use herdr_client::{DeviceKey, SessionRef};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Maximum persisted size, including the writer's trailing newline.
pub(crate) const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;

/// The original format, without paused sessions.
const VERSION_1: u32 = 1;
/// Adds `paused`. Written only while a session is paused, so a file without
/// stopped sessions stays readable by builds that know version 1 alone.
const VERSION_2: u32 = 2;

#[derive(Serialize, Deserialize)]
struct Member {
    device: String,
    session: String,
}

#[derive(Serialize, Deserialize)]
struct Window {
    id: ContainerId,
    name: String,
    members: Vec<Member>,
    selected: Option<Member>,
    grouping: Grouping,
}

#[derive(Serialize)]
struct Out {
    version: u32,
    windows: Vec<Window>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    paused: Vec<Member>,
}

#[derive(Deserialize)]
struct Versioned {
    version: u32,
}

#[derive(Deserialize)]
struct In {
    windows: Vec<Window>,
    #[serde(default)]
    paused: Vec<Member>,
}

fn member(session: &SessionRef) -> Member {
    Member {
        device: session.device.as_str().to_owned(),
        session: session.session.clone(),
    }
}

/// `None` for a member whose device key or session name is not valid.
fn session(member: Member) -> Option<SessionRef> {
    let device = DeviceKey::new(member.device).ok()?;
    valid_session(&member.session).then(|| SessionRef::new(device, member.session))
}

pub(super) fn encode(set: &ContainerSet) -> crate::Result<Vec<u8>> {
    let paused: Vec<Member> = set.paused().iter().map(member).collect();
    let out = Out {
        version: if paused.is_empty() {
            VERSION_1
        } else {
            VERSION_2
        },
        paused,
        windows: set
            .containers()
            .iter()
            .map(|c| Window {
                id: c.id,
                name: c.title.clone(),
                members: c.members.iter().map(member).collect(),
                selected: c.selected.as_ref().map(member),
                grouping: c.grouping,
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&out)?;
    if bytes.len() >= MAX_JSON_BYTES {
        return Err(ContainerError::InvalidState.into());
    }
    Ok(bytes)
}

/// Reads a file. A member that cannot be a [`SessionRef`] is dropped with a
/// warning rather than rejecting the file; everything else out of bounds
/// (version, size, counts, names, ids) rejects it.
pub(super) fn decode(bytes: &[u8]) -> Result<ContainerSet, ContainerError> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(ContainerError::InvalidState);
    }
    let versioned: Versioned =
        serde_json::from_slice(bytes).map_err(|_| ContainerError::InvalidState)?;
    if !matches!(versioned.version, VERSION_1 | VERSION_2) {
        return Err(ContainerError::InvalidState);
    }
    let file: In = serde_json::from_slice(bytes).map_err(|_| ContainerError::InvalidState)?;
    if file.windows.len() > MAX_CONTAINERS {
        return Err(ContainerError::InvalidState);
    }
    let mut containers = Vec::with_capacity(file.windows.len());
    for window in file.windows {
        if window.members.len() > MAX_SLOTS {
            return Err(ContainerError::InvalidState);
        }
        let mut seen = HashSet::new();
        let mut members = Vec::new();
        for raw in window.members {
            match session(raw) {
                Some(valid) => {
                    if seen.insert(valid.clone()) {
                        members.push(valid);
                    }
                }
                None => tracing::warn!(window = %window.name, "Dropping an invalid window member"),
            }
        }
        let selected = window
            .selected
            .and_then(session)
            .filter(|selected| members.contains(selected));
        containers.push(Container {
            id: window.id,
            title: window.name,
            members,
            selected,
            grouping: window.grouping,
        });
    }
    Ok(ContainerSet::validated(containers)?
        .with_paused(file.paused.into_iter().filter_map(session)))
}
