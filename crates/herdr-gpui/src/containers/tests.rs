#![allow(clippy::unwrap_used)]
use super::*;
use std::fs;

fn s(device: &str, session: &str) -> SessionRef {
    session_ref(device, session)
}

fn base() -> ContainerSet {
    let mut set = ContainerSet::default();
    set.ensure_default(Some(s("local", "default")));
    set
}

#[test]
fn a_default_window_lists_the_base_session() {
    let set = base();
    assert_eq!(set.containers().len(), 1);
    assert_eq!(set.containers()[0].title, "Window 1");
    assert_eq!(set.containers()[0].members, vec![s("local", "default")]);
    assert_eq!(set.containers()[0].grouping, Grouping::None);
}

#[test]
fn ids_are_stable_and_never_reused() {
    let mut set = base();
    let first = set.containers()[0].id;
    let second = set.create(None).unwrap();
    set.close(second).unwrap();
    let third = set.create(None).unwrap();
    assert_ne!(second, third);
    assert_eq!(set.containers()[0].id, first);
    assert_eq!(ContainerId::parse(&first.to_string()), Some(first));
}

#[test]
fn names_are_unique_bounded_and_printable() {
    let mut set = base();
    let id = set.create(Some("  Build ")).unwrap();
    assert_eq!(set.get(id).unwrap().title, "Build");
    assert_eq!(
        set.create(Some("Build")),
        Err(ContainerError::DuplicateTitle("Build".into()))
    );
    assert_eq!(set.create(Some("")), Err(ContainerError::InvalidTitle));
    assert_eq!(set.create(Some("a\nb")), Err(ContainerError::InvalidTitle));
    assert_eq!(
        set.create(Some(&"x".repeat(65))),
        Err(ContainerError::InvalidTitle)
    );
    assert!(set.create(Some(&"x".repeat(64))).is_ok());
    let other = set.create(None).unwrap();
    assert_eq!(
        set.rename(other, "Build"),
        Err(ContainerError::DuplicateTitle("Build".into()))
    );
    // Renaming to the Window's own name is not a clash.
    set.rename(id, "Build").unwrap();
    set.rename(other, "Ship").unwrap();
    assert_eq!(set.by_name("Ship"), Some(other));
}

#[test]
fn automatic_names_skip_taken_ones() {
    let mut set = base();
    set.create(Some("Window 2")).unwrap();
    let id = set.create(None).unwrap();
    assert_eq!(set.get(id).unwrap().title, "Window 3");
}

#[test]
fn the_window_count_and_member_count_are_bounded() {
    let mut set = ContainerSet::default();
    for n in 0..MAX_CONTAINERS {
        set.create(Some(&format!("w{n}"))).unwrap();
    }
    assert_eq!(
        set.create(Some("one more")),
        Err(ContainerError::TooManyContainers(MAX_CONTAINERS))
    );
    let id = set.containers()[0].id;
    for n in 0..MAX_SLOTS {
        set.attach(id, s("ssh:a", &format!("s{n}"))).unwrap();
    }
    assert_eq!(
        set.attach(id, s("ssh:a", "overflow")),
        Err(ContainerError::TooManySlots(MAX_SLOTS))
    );
    // A member already listed is selected, not refused.
    assert_eq!(set.attach(id, s("ssh:a", "s0")), Ok(false));
}

#[test]
fn closing_the_last_window_keeps_an_empty_default() {
    let mut set = base();
    let only = set.containers()[0].id;
    assert!(set.detach(only, &s("local", "default")));
    set.close(only).unwrap();
    assert_eq!(set.containers().len(), 1);
    assert_ne!(set.containers()[0].id, only);
    assert!(set.containers()[0].members.is_empty());
    assert_eq!(set.close(only), Err(ContainerError::NoSuchContainer));
}

#[test]
fn a_window_that_lists_sessions_cannot_be_closed() {
    let mut set = base();
    let only = set.containers()[0].id;
    assert_eq!(set.close(only), Err(ContainerError::NotEmpty));
    assert_eq!(set.get(only).unwrap().members, vec![s("local", "default")]);
}

#[test]
fn attach_dedupes_within_a_window_but_not_across() {
    let mut set = base();
    let first = set.containers()[0].id;
    assert_eq!(set.attach(first, s("ssh:a", "work")), Ok(true));
    assert_eq!(set.attach(first, s("ssh:a", "work")), Ok(false));
    let second = set.create(Some("Other")).unwrap();
    assert_eq!(set.attach(second, s("ssh:a", "work")), Ok(true));
    // Same name on another device is a different session.
    assert_eq!(set.attach(second, s("ssh:b", "work")), Ok(true));
    assert_eq!(set.sessions().count(), 3);
}

#[test]
fn detach_and_move_keep_order_and_selection() {
    let mut set = base();
    let id = set.containers()[0].id;
    for name in ["a", "b", "c"] {
        set.attach(id, s("ssh:x", name)).unwrap();
    }
    set.select(id, &s("ssh:x", "b"));
    assert!(set.detach(id, &s("ssh:x", "b")));
    assert_eq!(set.get(id).unwrap().selected, Some(s("ssh:x", "c")));
    assert!(!set.detach(id, &s("ssh:x", "b")));
    set.move_member(id, &s("ssh:x", "c"), 0).unwrap();
    let order: Vec<_> = set
        .get(id)
        .unwrap()
        .members
        .iter()
        .map(|m| m.session.clone())
        .collect();
    assert_eq!(order, ["c", "default", "a"]);
    assert_eq!(
        set.move_member(id, &s("ssh:x", "gone"), 0),
        Err(ContainerError::NoSuchMember)
    );
    let second = set.create(None).unwrap();
    set.move_container(second, 0).unwrap();
    assert_eq!(set.containers()[0].id, second);
}

#[test]
fn deleting_a_session_rewrites_every_window_to_the_default_once() {
    let mut set = base();
    let first = set.containers()[0].id;
    let second = set.create(None).unwrap();
    for id in [first, second] {
        set.attach(id, s("ssh:a", "gone")).unwrap();
        set.attach(id, s("ssh:a", "default")).unwrap();
        set.attach(id, s("ssh:b", "gone")).unwrap();
        set.select(id, &s("ssh:a", "gone"));
    }
    set.replace_session(&s("ssh:a", "").device, "gone", "default");
    for id in [first, second] {
        let window = set.get(id).unwrap();
        assert_eq!(
            window
                .members
                .iter()
                .filter(|m| **m == s("ssh:a", "default"))
                .count(),
            1
        );
        assert!(
            window.members.contains(&s("ssh:b", "gone")),
            "other devices stay"
        );
        assert!(!window.members.contains(&s("ssh:a", "gone")));
        assert_eq!(window.selected, Some(s("ssh:a", "default")));
    }
}

#[test]
fn retain_moves_the_selection_to_a_neighbor() {
    let mut set = base();
    let id = set.containers()[0].id;
    set.attach(id, s("ssh:gone", "x")).unwrap();
    set.retain(|m| m.device.as_str() != "ssh:gone");
    assert_eq!(set.get(id).unwrap().selected, Some(s("local", "default")));
}

fn rich() -> ContainerSet {
    let mut set = base();
    let id = set.containers()[0].id;
    set.attach(id, s("ssh:box", "work")).unwrap();
    let second = set.create(Some("Review")).unwrap();
    set.attach(second, s("ssh:box", "work")).unwrap();
    set.set_grouping(second, Grouping::Project).unwrap();
    set
}

#[test]
fn the_file_round_trips_deterministically() {
    let set = rich();
    let bytes = file::encode(&set).unwrap();
    assert_eq!(file::decode(&bytes).unwrap(), set);
    assert_eq!(file::encode(&set).unwrap(), bytes);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["windows"][1]["grouping"], "project");
}

#[test]
fn unreadable_oversized_and_unknown_version_files_are_refused() {
    assert!(file::decode(b"broken").is_err());
    assert!(file::decode(br#"{"version": 3, "windows": []}"#).is_err());
    assert!(file::decode(br#"{"windows": []}"#).is_err());
    let huge = vec![b' '; file::MAX_JSON_BYTES + 1];
    assert!(file::decode(&huge).is_err());
    // Duplicate names are not a valid registry.
    let text = |names: &[&str]| {
        let windows: Vec<_> = names
            .iter()
            .map(|n| {
                serde_json::json!({"id": ContainerId::new(), "name": n, "members": [],
                    "selected": null, "grouping": "none"})
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({"version": 1, "windows": windows})).unwrap()
    };
    assert!(file::decode(&text(&["a", "b"])).is_ok());
    assert!(file::decode(&text(&["a", "a"])).is_err());
    let many: Vec<String> = (0..=MAX_CONTAINERS).map(|n| format!("w{n}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(file::decode(&text(&refs)).is_err());
}

#[test]
fn an_invalid_member_is_dropped_without_rejecting_the_file() {
    let id = ContainerId::new();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "windows": [{
            "id": id, "name": "w", "grouping": "none",
            "members": [
                {"device": "", "session": "x"},
                {"device": "ssh:ok", "session": "y"},
                {"device": "ssh:ok", "session": "bad\nname"},
            ],
            "selected": {"device": "", "session": "x"},
        }],
    }))
    .unwrap();
    let set = file::decode(&bytes).unwrap();
    let window = set.get(id).unwrap();
    assert_eq!(window.members, vec![s("ssh:ok", "y")]);
    assert_eq!(window.selected, None);
}

fn legacy_file() -> Vec<u8> {
    let window = |title: &str, device: &str, session: &str| {
        serde_json::json!({
            "id": 1, "title": title,
            "slots": [{"device": device, "session": session}],
            "selected": {"device": device, "session": session},
        })
    };
    let native = |titles: [&str; 2]| {
        serde_json::json!({
            "containers": [window(titles[0], "local", "default"), window(titles[1], "ssh:a", "w")],
            "active": 1, "next_id": 3,
        })
    };
    // Both native windows used the same names; a third entry restored nothing.
    serde_json::to_vec(&serde_json::json!([
        native(["Window 1", "Work"]),
        native(["Window 1", "Work"]),
        null
    ]))
    .unwrap()
}

#[test]
fn the_old_per_window_file_becomes_global_windows_with_numbered_duplicates() {
    let migrated = legacy::migrate(&legacy_file()).unwrap();
    let titles: Vec<_> = migrated
        .set
        .containers()
        .iter()
        .map(|c| c.title.as_str())
        .collect();
    assert_eq!(titles, ["Window 1", "Work", "Window 1 2", "Work 2"]);
    assert_eq!(migrated.shown.len(), 3);
    // Each native window keeps showing the Window that was active in it.
    assert_eq!(migrated.shown[0], Some(migrated.set.containers()[1].id));
    assert_eq!(migrated.shown[1], Some(migrated.set.containers()[3].id));
    assert_eq!(migrated.shown[2], None);
    assert!(file::decode(&file::encode(&migrated.set).unwrap()).is_ok());
}

#[test]
fn startup_migrates_once_and_retires_the_old_file() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("window-containers.json");
    fs::write(&old, legacy_file()).unwrap();
    let path = dir.path().join("windows.json");
    let mut store = Containers::at(Some(path.clone()));
    assert_eq!(store.set().containers().len(), 4);
    assert!(store.take_migrated_shown().is_some());
    assert!(store.take_migrated_shown().is_none());
    assert!(path.exists());
    assert!(!old.exists());
    assert!(dir.path().join("window-containers.json.migrated").exists());
    // The next start reads windows.json and migrates nothing.
    drop(store);
    let again = Containers::at(Some(path));
    assert_eq!(again.set().containers().len(), 4);
}

#[test]
fn an_existing_registry_is_not_overwritten_by_the_old_file() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("window-containers.json");
    fs::write(&old, legacy_file()).unwrap();
    let path = dir.path().join("windows.json");
    fs::write(&path, file::encode(&base()).unwrap()).unwrap();
    let mut store = Containers::at(Some(path));
    assert_eq!(store.set().containers().len(), 1);
    assert!(store.take_migrated_shown().is_none());
    assert!(old.exists());
}

#[test]
fn a_damaged_registry_starts_empty_and_is_preserved_on_the_first_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("windows.json");
    fs::write(&path, b"broken").unwrap();
    let mut store = Containers::at(Some(path.clone()));
    assert!(store.set().containers().is_empty());
    // Untouched until something changes.
    assert_eq!(fs::read(&path).unwrap(), b"broken");
    store
        .mutate(|set| {
            set.ensure_default(None);
            Ok(())
        })
        .unwrap();
    store.finish();
    let backup = dir.path().join("windows.json.bad");
    assert_eq!(fs::read(&backup).unwrap(), b"broken");
    assert!(file::decode(&fs::read(&path).unwrap()).is_ok());
}

#[test]
fn a_failed_change_leaves_the_registry_and_revision_alone() {
    let mut store = Containers::default();
    store
        .mutate(|set| {
            set.ensure_default(None);
            Ok(())
        })
        .unwrap();
    let revision = store.revision();
    let id = store.set().containers()[0].id;
    assert!(store.mutate(|set| set.rename(id, "")).is_err());
    assert_eq!(store.revision(), revision);
    // A change that changes nothing is not a revision.
    store.mutate(|_| Ok(())).unwrap();
    assert_eq!(store.revision(), revision);
    store.mutate(|set| set.rename(id, "New")).unwrap();
    assert_eq!(store.revision(), revision + 1);
}

mod concurrent;
