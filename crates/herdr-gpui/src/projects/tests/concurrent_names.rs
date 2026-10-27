use super::*;

#[test]
fn a_project_both_processes_added_with_one_name_is_rejected_without_losing_links() {
    let base = Registry::new();
    let mut theirs = base.clone();
    let saved = theirs.create_project("App", None).unwrap();
    let mut mine = base.clone();
    let ghost = mine.create_project("app", None).unwrap();
    mine.assign(
        SessionRef::new(DeviceKey::new(device()).unwrap(), "s1"),
        &ghost.id,
    )
    .unwrap();
    assert!(matches!(
        Registry::merge(&base, &mine, &theirs),
        Err(ProjectError::DuplicateName(_))
    ));
    assert!(theirs.project(&saved.id).is_some());
    assert!(mine.project(&ghost.id).is_some());
    assert_eq!(mine.projects().len(), 1);
    assert!(
        mine.assignment(&SessionRef::new(DeviceKey::new(device()).unwrap(), "s1"))
            .is_some()
    );
}
