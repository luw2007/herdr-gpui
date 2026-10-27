#![allow(clippy::unwrap_used)]
use super::*;
use herdr_client::Registry;
use std::sync::Arc;

#[test]
fn a_rejected_snapshot_followed_by_a_queued_edit_keeps_disk_only_projects() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let base = Registry::new();
    let mut theirs = base.clone();
    let disk_project = theirs.create_project("Same", None).unwrap();
    state_file::write(&path, &Snapshot(theirs)).unwrap();
    let mut mine = base.clone();
    mine.create_project("Same", None).unwrap();
    let inbox = Arc::new(Inbox::default());
    let mut save = saver(base, None, inbox.clone());
    assert!(matches!(
        save(&path, Snapshot(mine.clone())),
        Err(crate::Error::Project(
            herdr_client::ProjectError::DuplicateName(_)
        ))
    ));
    let added = mine.create_project("Later", None).unwrap();
    save(&path, Snapshot(mine)).unwrap();
    let disk = Registry::from_json(&std::fs::read(path).unwrap()).unwrap();
    assert!(
        disk.project(&disk_project.id).is_some(),
        "rejected snapshot is not a deletion of disk-only state"
    );
    assert!(disk.project(&added.id).is_some());
    assert_eq!(disk.projects().len(), 2);
}

#[test]
fn adoption_of_conflicting_newer_edits_resolves_to_the_valid_saved_registry() {
    let base = Registry::new();
    let mut saved = base.clone();
    saved.create_project("Same", None).unwrap();
    let mut merged = base.clone();
    let kept = merged.create_project("Same", None).unwrap();
    let mut current = saved.clone();
    current.create_project("Later", None).unwrap();
    // A newly edited name collides again with an independent disk addition.
    current.create_project("Collision", None).unwrap();
    merged.create_project("Collision", None).unwrap();
    let inbox = Inbox::default();
    inbox.publish(saved, merged.clone());
    let (next, dirty) = adopt(&inbox, &current).unwrap().unwrap();
    assert_eq!(next, merged);
    assert!(next.project(&kept.id).is_some());
    assert!(!dirty);
    assert!(!inbox.has_news());
}
