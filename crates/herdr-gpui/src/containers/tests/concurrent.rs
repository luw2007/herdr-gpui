use super::*;
use std::time::{Duration, Instant};

fn await_window(path: &std::path::Path, name: &str) -> ContainerSet {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(bytes) = fs::read(path)
            && let Ok(set) = file::decode(&bytes)
            && set.by_name(name).is_some()
        {
            return set;
        }
        assert!(
            Instant::now() < deadline,
            "windows writer never saved {name}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn independent_windows_stores_alternate_without_losing_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("windows.json");
    let mut first = Containers::at(Some(path.clone()));
    let mut second = Containers::at(Some(path.clone()));
    first.mutate(|set| set.create(Some("First"))).unwrap();
    await_window(&path, "First");
    second.mutate(|set| set.create(Some("Second"))).unwrap();
    let disk = await_window(&path, "Second");
    assert!(disk.by_name("First").is_some());
    // First still holds its original local snapshot: the import need not have
    // been adopted on the UI thread for the next save to preserve Second.
    first.mutate(|set| set.create(Some("Third"))).unwrap();
    let disk = await_window(&path, "Third");
    assert!(disk.by_name("First").is_some());
    assert!(disk.by_name("Second").is_some());
    first.finish();
    second.finish();
}

#[test]
fn concurrent_window_fields_and_members_merge_without_replacing_other_windows() {
    let mut base = base();
    let id = base.containers()[0].id;
    let mut mine = base.clone();
    let mut theirs = base.clone();
    mine.rename(id, "Mine").unwrap();
    theirs.attach(id, s("ssh:other", "work")).unwrap();
    let merged = ContainerSet::merge(&base, &mine, &theirs).unwrap();
    assert_eq!(merged.get(id).unwrap().title, "Mine");
    assert!(
        merged
            .get(id)
            .unwrap()
            .members
            .contains(&s("ssh:other", "work"))
    );
    base.pause(s("ssh:box", "paused"));
    let mut resumed = base.clone();
    resumed.resume(&s("ssh:box", "paused"));
    let mut stopped = base.clone();
    stopped.pause(s("ssh:other", "stopped"));
    let merged = ContainerSet::merge(&base, &resumed, &stopped).unwrap();
    assert!(!merged.is_paused(&s("ssh:box", "paused")));
    assert!(merged.is_paused(&s("ssh:other", "stopped")));
}

#[test]
fn closing_an_empty_window_never_discards_a_concurrent_member() {
    let mut base = base();
    let id = base.create(Some("Concurrent")).unwrap();
    let mut closed = base.clone();
    closed.close(id).unwrap();
    let mut attached = base.clone();
    let member = s("ssh:other", "work");
    attached.attach(id, member.clone()).unwrap();
    for merged in [
        ContainerSet::merge(&base, &closed, &attached).unwrap(),
        ContainerSet::merge(&base, &attached, &closed).unwrap(),
    ] {
        assert_eq!(merged.get(id).unwrap().members, vec![member.clone()]);
        assert_eq!(merged.get(id).unwrap().selected, Some(member.clone()));
        file::decode(&file::encode(&merged).unwrap()).unwrap();
    }
}

#[test]
fn closing_an_empty_window_wins_over_a_concurrent_rename_in_either_order() {
    let mut base = base();
    let id = base.create(Some("Concurrent")).unwrap();
    let mut closed = base.clone();
    closed.close(id).unwrap();
    let mut renamed = base.clone();
    renamed.rename(id, "Renamed").unwrap();
    for merged in [
        ContainerSet::merge(&base, &closed, &renamed).unwrap(),
        ContainerSet::merge(&base, &renamed, &closed).unwrap(),
    ] {
        assert!(
            merged.get(id).is_none(),
            "rename must not resurrect a closed Window"
        );
        file::decode(&file::encode(&merged).unwrap()).unwrap();
    }
}

#[test]
fn concurrent_windows_at_the_limit_are_rejected_not_silently_dropped() {
    let mut base = base();
    for n in 1..MAX_CONTAINERS - 1 {
        base.create(Some(&format!("Extra {n}"))).unwrap();
    }
    let mut mine = base.clone();
    let mut theirs = base.clone();
    let mine_id = mine.create(Some("Mine")).unwrap();
    let theirs_id = theirs.create(Some("Theirs")).unwrap();
    mine.attach(mine_id, s("local", "mine")).unwrap();
    theirs.attach(theirs_id, s("local", "theirs")).unwrap();
    assert_eq!(
        ContainerSet::merge(&base, &mine, &theirs),
        Err(ContainerError::TooManyContainers(MAX_CONTAINERS))
    );
}

#[test]
fn concurrent_members_at_the_limit_are_rejected_not_silently_detached() {
    let mut base = base();
    let id = base.containers()[0].id;
    for n in 1..MAX_SLOTS - 1 {
        base.attach(id, s("local", &format!("session-{n}")))
            .unwrap();
    }
    let mut mine = base.clone();
    let mut theirs = base.clone();
    mine.attach(id, s("local", "mine")).unwrap();
    theirs.attach(id, s("local", "theirs")).unwrap();
    assert_eq!(
        ContainerSet::merge(&base, &mine, &theirs),
        Err(ContainerError::TooManySlots(MAX_SLOTS))
    );
}

#[test]
fn a_file_damaged_after_loading_is_preserved_before_the_writer_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("windows.json");
    let mut store = Containers::at(Some(path.clone()));
    store.mutate(|set| set.create(Some("Before"))).unwrap();
    await_window(&path, "Before");
    fs::write(&path, b"{damaged while open").unwrap();
    store.mutate(|set| set.create(Some("After"))).unwrap();
    store.finish();
    let disk = file::decode(&fs::read(&path).unwrap()).unwrap();
    assert!(disk.by_name("Before").is_some());
    assert!(disk.by_name("After").is_some());
    assert_eq!(
        fs::read(dir.path().join("windows.json.bad")).unwrap(),
        b"{damaged while open"
    );
}

#[test]
fn concurrent_same_name_windows_keep_both_ids_with_unique_titles() {
    let base = base();
    let mut mine = base.clone();
    let mut theirs = base.clone();
    let mine_id = mine.create(Some("Shared")).unwrap();
    let theirs_id = theirs.create(Some("Shared")).unwrap();
    let merged = ContainerSet::merge(&base, &mine, &theirs).unwrap();
    assert!(merged.get(mine_id).is_some());
    assert!(merged.get(theirs_id).is_some());
    assert_ne!(
        merged.get(mine_id).unwrap().title,
        merged.get(theirs_id).unwrap().title
    );
    assert_eq!(merged.containers().len(), 3);
    file::decode(&file::encode(&merged).unwrap()).unwrap();
}

#[test]
fn member_reordering_survives_merge_and_keeps_concurrent_additions() {
    let mut base = base();
    let id = base.containers()[0].id;
    let second = s("local", "second");
    let added = s("local", "added");
    base.attach(id, second.clone()).unwrap();
    let mut mine = base.clone();
    mine.move_member(id, &second, 0).unwrap();
    let plain = ContainerSet::merge(&base, &mine, &base).unwrap();
    assert_eq!(
        plain.get(id).unwrap().members,
        mine.get(id).unwrap().members
    );
    let mut theirs = base.clone();
    theirs.attach(id, added.clone()).unwrap();
    let merged = ContainerSet::merge(&base, &mine, &theirs).unwrap();
    assert_eq!(
        merged.get(id).unwrap().members,
        vec![second, s("local", "default"), added]
    );
}

#[test]
fn count_valid_windows_over_the_encoded_byte_cap_are_refused() {
    let mut set = ContainerSet::default();
    let device = "🦀".repeat(128);
    for window in 0..MAX_CONTAINERS {
        let id = set.create(Some(&format!("Window {window}"))).unwrap();
        for member in 0..MAX_SLOTS {
            let name = format!("{member:02}{}", "\\".repeat(1022));
            set.attach(id, s(&device, &name)).unwrap();
        }
    }
    assert!(
        file::encode(&set).is_err(),
        "valid counts do not imply valid encoded size"
    );
    assert_eq!(
        ContainerSet::merge(&set, &set, &set),
        Err(ContainerError::InvalidState)
    );
    let mut store = Containers::default();
    assert_eq!(
        store.mutate(|next| {
            *next = set;
            Ok(())
        }),
        Err(ContainerError::InvalidState)
    );
    assert!(
        store.set().containers().is_empty(),
        "failed mutation must be atomic"
    );
}
