//! Editing an association is one mutation: failures must not remove the old
//! folder or advance the persisted store revision.
use super::*;

#[test]
fn replacing_a_binding_with_an_invalid_path_or_device_changes_nothing() {
    let mut store = Projects::default();
    let project = store.create_project("App", None).unwrap();
    let previous = store.bind(device(), "/work/app", &project.id).unwrap();
    let before = (store.registry().clone(), store.revision());

    assert!(matches!(
        store.replace_binding(Some(&previous), device(), "relative/path", &project.id),
        Err(crate::Error::Project(ProjectError::InvalidPath))
    ));
    assert_eq!(store.registry(), &before.0);
    assert_eq!(store.revision(), before.1);

    assert!(matches!(
        store.replace_binding(Some(&previous), "", "/work/new", &project.id),
        Err(crate::Error::Project(ProjectError::InvalidDevice))
    ));
    assert_eq!(store.registry(), &before.0);
    assert_eq!(store.revision(), before.1);
}

#[test]
fn replacing_a_binding_with_another_projects_path_preserves_both_associations() {
    let mut store = Projects::default();
    let app = store.create_project("App", None).unwrap();
    let other = store.create_project("Other", None).unwrap();
    let previous = store.bind(device(), "/work/app", &app.id).unwrap();
    store.bind(device(), "/work/other", &other.id).unwrap();
    let before = (store.registry().clone(), store.revision());

    assert!(matches!(
        store.replace_binding(Some(&previous), device(), "/work/other", &app.id),
        Err(crate::Error::Project(ProjectError::BindingConflict { existing, .. }))
            if existing == other.id
    ));
    assert_eq!(store.registry(), &before.0);
    assert_eq!(store.revision(), before.1);
}

#[test]
fn replacing_a_binding_for_a_deleted_project_changes_nothing() {
    let mut store = Projects::default();
    let app = store.create_project("App", None).unwrap();
    let deleted = store.create_project("Deleted", None).unwrap();
    let previous = store.bind(device(), "/work/app", &app.id).unwrap();
    store.remove_project(&deleted.id).unwrap();
    let before = (store.registry().clone(), store.revision());

    assert!(matches!(
        store.replace_binding(Some(&previous), device(), "/work/new", &deleted.id),
        Err(crate::Error::Project(ProjectError::UnknownProject(id))) if id == deleted.id
    ));
    assert_eq!(store.registry(), &before.0);
    assert_eq!(store.revision(), before.1);
}

#[test]
fn a_valid_replacement_changes_device_and_path_in_one_revision() {
    let mut store = Projects::default();
    let app = store.create_project("App", None).unwrap();
    let previous = store.bind(device(), "/work/app", &app.id).unwrap();
    let revision = store.revision();

    store
        .replace_binding(Some(&previous), "ssh:other", "/srv/app", &app.id)
        .unwrap();
    let bindings = store.registry().bindings_of(&app.id);
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].device.as_str(), "ssh:other");
    assert_eq!(bindings[0].path, "/srv/app");
    assert_eq!(store.revision(), revision + 1);
    assert!(!bindings.contains(&previous));
}

#[test]
fn a_stale_edit_does_not_remove_another_projects_new_association() {
    let mut store = Projects::default();
    let app = store.create_project("App", None).unwrap();
    let other = store.create_project("Other", None).unwrap();
    let previous = store.bind(device(), "/work/app", &app.id).unwrap();
    store.unbind(device(), "/work/app").unwrap();
    let reassigned = store.bind(device(), "/work/app", &other.id).unwrap();

    store
        .replace_binding(Some(&previous), device(), "/work/new", &app.id)
        .unwrap();
    assert_eq!(store.registry().bindings_of(&other.id), vec![reassigned]);
    let bindings = store.registry().bindings_of(&app.id);
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].path, "/work/new");
}
