#![allow(clippy::unwrap_used)]
use super::*;

fn s(device: &str, session: &str) -> SessionRef {
    SessionRef::new(device, session)
}

#[test]
fn a_new_container_is_empty_and_active() {
    let mut set = ContainerSet::new(s("local", "default"));
    assert_eq!(set.active().slots.len(), 1);
    let index = set.create(None).unwrap();
    assert_eq!(set.active_index(), index);
    assert!(set.active().slots.is_empty());
    assert_eq!(set.containers().len(), 2);
}

#[test]
fn attach_dedupes_within_a_container_but_not_across() {
    let mut set = ContainerSet::new(s("local", "default"));
    assert!(set.attach(0, s("ssh:a", "work")).unwrap());
    assert!(!set.attach(0, s("ssh:a", "work")).unwrap());
    assert_eq!(set.containers()[0].slots.len(), 2);
    let second = set.create(Some("Other")).unwrap();
    assert!(set.attach(second, s("ssh:a", "work")).unwrap());
    // Same device, different session: a distinct row.
    assert!(set.attach(second, s("ssh:a", "other")).unwrap());
    assert_eq!(set.containers()[second].slots.len(), 2);
    assert_eq!(set.sessions().count(), 3);
}

#[test]
fn same_device_sessions_coexist_with_independent_selection() {
    let mut set = ContainerSet::new(s("ssh:a", "one"));
    set.attach(0, s("ssh:a", "two")).unwrap();
    assert_eq!(set.active().selected, Some(s("ssh:a", "two")));
    assert!(set.select(&s("ssh:a", "one")));
    assert_eq!(set.active().slots.len(), 2);
}

#[test]
fn detach_is_view_only_and_moves_selection_to_a_neighbor() {
    let mut set = ContainerSet::new(s("local", "a"));
    set.attach(0, s("local", "b")).unwrap();
    set.attach(0, s("local", "c")).unwrap();
    set.select(&s("local", "b"));
    assert!(set.detach(0, &s("local", "b")));
    assert_eq!(set.active().selected, Some(s("local", "c")));
    assert!(!set.detach(0, &s("local", "b")));
    // The other container still lists what another detached.
    let other = set.create(None).unwrap();
    set.attach(other, s("local", "a")).unwrap();
    set.detach(0, &s("local", "a"));
    assert!(set.references(&s("local", "a")));
}

#[test]
fn closing_the_last_container_leaves_an_empty_replacement() {
    let mut set = ContainerSet::new(s("local", "default"));
    set.close(0).unwrap();
    assert_eq!(set.containers().len(), 1);
    assert!(set.active().slots.is_empty());
    assert_eq!(set.close(5), Err(ContainerError::NoSuchContainer));
}

#[test]
fn closing_keeps_the_active_one_in_range() {
    let mut set = ContainerSet::new(s("local", "a"));
    set.create(None).unwrap();
    set.create(None).unwrap();
    assert_eq!(set.active_index(), 2);
    set.close(2).unwrap();
    assert_eq!(set.active_index(), 1);
    set.activate(0);
    set.close(0).unwrap();
    assert_eq!(set.active_index(), 0);
    assert_eq!(set.containers().len(), 1);
}

#[test]
fn stepping_wraps() {
    let mut set = ContainerSet::new(s("local", "a"));
    set.create(None).unwrap();
    set.step(true);
    assert_eq!(set.active_index(), 0);
    set.step(false);
    assert_eq!(set.active_index(), 1);
}

#[test]
fn deleted_sessions_are_replaced_everywhere_without_duplicates() {
    let mut set = ContainerSet::new(s("ssh:a", "default"));
    set.attach(0, s("ssh:a", "gone")).unwrap();
    let other = set.create(None).unwrap();
    set.attach(other, s("ssh:a", "gone")).unwrap();
    set.replace_session("ssh:a", "gone", "default");
    assert_eq!(set.containers()[0].slots, vec![s("ssh:a", "default")]);
    assert_eq!(set.containers()[other].slots, vec![s("ssh:a", "default")]);
    assert_eq!(
        set.containers()[other].selected,
        Some(s("ssh:a", "default"))
    );
}

#[test]
fn limits_are_enforced() {
    let mut set = ContainerSet::new(s("local", "a"));
    for _ in 1..MAX_CONTAINERS {
        set.create(None).unwrap();
    }
    assert_eq!(
        set.create(None),
        Err(ContainerError::TooManyContainers(MAX_CONTAINERS))
    );
    for n in 0..MAX_SLOTS {
        let _ = set.attach(0, s("local", &format!("s{n}")));
    }
    assert_eq!(
        set.attach(0, s("local", "overflow")),
        Err(ContainerError::TooManySlots(MAX_SLOTS))
    );
    assert_eq!(
        set.create(None),
        Err(ContainerError::TooManyContainers(MAX_CONTAINERS))
    );
}

#[test]
fn titles_are_bounded_and_printable() {
    assert_eq!(clean_title("  ok  ").unwrap(), "ok");
    assert!(clean_title("").is_err());
    assert!(clean_title("bad\u{7}").is_err());
    assert!(clean_title(&"x".repeat(65)).is_err());
}

#[test]
fn restored_data_is_validated() {
    let good = ContainerSet::new(s("local", "default"));
    let json = serde_json::to_string(&good).unwrap();
    let back: ContainerSet = serde_json::from_str(&json).unwrap();
    assert_eq!(back.validate().unwrap(), good);

    let mut dup = good.clone();
    dup.containers[0].slots.push(s("local", "default"));
    assert_eq!(dup.validate(), Err(ContainerError::InvalidState));

    let mut stray = good.clone();
    stray.containers[0].selected = Some(s("local", "elsewhere"));
    assert_eq!(stray.validate(), Err(ContainerError::InvalidState));

    let mut active = good.clone();
    active.active = 3;
    assert_eq!(active.validate(), Err(ContainerError::InvalidState));

    let mut empty = good.clone();
    empty.containers.clear();
    assert_eq!(empty.validate(), Err(ContainerError::InvalidState));

    let mut ids = good.clone();
    ids.containers.push(ids.containers[0].clone());
    assert_eq!(ids.validate(), Err(ContainerError::InvalidState));

    let mut title = good.clone();
    title.containers[0].title = "bad\n".into();
    assert_eq!(title.validate(), Err(ContainerError::InvalidState));

    let mut device = good.clone();
    device.containers[0].slots[0].device = String::new();
    device.containers[0].selected = None;
    assert_eq!(device.validate(), Err(ContainerError::InvalidState));
}

#[test]
fn retain_drops_slots_and_repairs_selection() {
    let mut set = ContainerSet::new(s("ssh:gone", "a"));
    set.attach(0, s("local", "b")).unwrap();
    set.select(&s("ssh:gone", "a"));
    set.retain(|slot| slot.device != "ssh:gone");
    assert_eq!(set.active().slots, vec![s("local", "b")]);
    assert_eq!(set.active().selected, Some(s("local", "b")));
}
