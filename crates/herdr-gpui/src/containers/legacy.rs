//! The retired per-native-window `window-containers.json`: a list parallel to
//! the window geometry list, entry `n` being the `n`th native window's own
//! Windows. It is read once, to seed the global registry.
use super::{
    Container, ContainerId, ContainerSet, Grouping, MAX_CONTAINERS, MAX_SLOTS, MAX_TITLE,
    clean_title, valid_session,
};
use herdr_client::{DeviceKey, SessionRef};
use serde::Deserialize;
use std::collections::HashSet;

/// The most the old file was ever allowed to hold.
pub(super) const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Slot {
    device: String,
    session: String,
}

#[derive(Deserialize)]
struct Old {
    title: String,
    slots: Vec<Slot>,
    #[serde(default)]
    selected: Option<Slot>,
}

#[derive(Deserialize)]
struct OldSet {
    containers: Vec<Old>,
    active: usize,
}

pub(super) struct Migrated {
    pub set: ContainerSet,
    /// The Window each native window showed, in native window order.
    pub shown: Vec<Option<ContainerId>>,
}

fn session(slot: Slot) -> Option<SessionRef> {
    let device = DeviceKey::new(slot.device).ok()?;
    valid_session(&slot.session).then(|| SessionRef::new(device, slot.session))
}

/// `title`, or `title 2`, `title 3`, ... when a Window already has the name.
pub(super) fn unique(title: &str, taken: &HashSet<String>) -> String {
    let base = clean_title(title).unwrap_or_else(|_| "Space".to_owned());
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|n| {
            let suffix = format!(" {n}");
            let keep = MAX_TITLE - suffix.chars().count();
            format!("{}{suffix}", base.chars().take(keep).collect::<String>())
        })
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or(base)
}

pub(super) fn migrate(bytes: &[u8]) -> Result<Migrated, serde_json::Error> {
    let old: Vec<Option<OldSet>> = serde_json::from_slice(bytes)?;
    let mut containers: Vec<Container> = Vec::new();
    let mut taken = HashSet::new();
    let mut shown = Vec::with_capacity(old.len());
    for native in old {
        let Some(native) = native else {
            shown.push(None);
            continue;
        };
        let mut active = None;
        for (index, window) in native.containers.into_iter().enumerate() {
            if containers.len() >= MAX_CONTAINERS {
                tracing::warn!("Dropping saved windows beyond the limit");
                break;
            }
            let mut seen = HashSet::new();
            let members: Vec<SessionRef> = window
                .slots
                .into_iter()
                .filter_map(session)
                .filter(|member| seen.insert(member.clone()))
                .take(MAX_SLOTS)
                .collect();
            let selected = window
                .selected
                .and_then(session)
                .filter(|selected| members.contains(selected));
            let title = unique(&window.title, &taken);
            taken.insert(title.clone());
            let id = ContainerId::new();
            if index == native.active {
                active = Some(id);
            }
            containers.push(Container {
                id,
                title,
                members,
                selected,
                grouping: Grouping::None,
            });
        }
        shown.push(active);
    }
    let set = ContainerSet {
        containers,
        paused: Vec::new(),
    };
    Ok(Migrated { set, shown })
}
