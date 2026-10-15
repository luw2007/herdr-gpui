use super::*;

fn saved(store: Projects) {
    store.writer.unwrap().finish();
}

#[test]
fn a_missing_file_is_empty_and_a_change_round_trips_through_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/projects.json");
    let mut store = Projects::at(Some(path.clone()));
    assert!(store.registry().is_empty());
    let project = store.create_project("Billing", None).unwrap();
    store.bind(device(), "/work/billing", &project.id).unwrap();
    store.assign(device(), "s1", &project.id).unwrap();
    saved(store);
    let again = Projects::at(Some(path));
    assert_eq!(again.registry().projects().len(), 1);
    let resolution = again.resolve_session(device(), "s1", &[]).unwrap();
    assert_eq!(resolution.project, Some(project.id));
    assert_eq!(
        again
            .registry()
            .bindings_of(&resolution.project.unwrap())
            .len(),
        1
    );
}

#[test]
fn a_corrupt_file_starts_empty_is_kept_until_a_change_and_then_backed_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    std::fs::write(&path, b"{ not json").unwrap();
    let store = Projects::at(Some(path.clone()));
    assert!(store.registry().is_empty());
    // Loading alone, even finishing the writer, leaves the bad file alone.
    saved(store);
    assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
    assert!(!dir.path().join("projects.json.bad").exists());

    let mut store = Projects::at(Some(path.clone()));
    store.create_project("Fresh", None).unwrap();
    saved(store);
    assert_eq!(
        std::fs::read(dir.path().join("projects.json.bad")).unwrap(),
        b"{ not json"
    );
    assert_eq!(Projects::at(Some(path)).registry().projects().len(), 1);
}

#[test]
fn oversized_and_unsupported_files_are_treated_as_damaged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    std::fs::write(
        &path,
        br#"{"version":99,"projects":[],"bindings":[],"assignments":[]}"#,
    )
    .unwrap();
    assert!(Projects::at(Some(path.clone())).registry().is_empty());
    std::fs::write(&path, vec![b' '; Registry::MAX_JSON_BYTES + 1]).unwrap();
    let store = Projects::at(Some(path.clone()));
    assert!(store.registry().is_empty());
    saved(store);
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        Registry::MAX_JSON_BYTES as u64 + 1
    );
}
