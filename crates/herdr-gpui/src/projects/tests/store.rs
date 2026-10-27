use super::*;
use gpui::TestAppContext;

#[test]
fn mutations_bump_the_revision_and_failures_change_nothing() {
    let mut store = Projects::default();
    assert_eq!(store.revision(), 0);
    let project = store.create_project("App", None).unwrap();
    assert_eq!(store.revision(), 1);
    assert!(matches!(
        store.create_project("app", None),
        Err(crate::Error::Project(ProjectError::DuplicateName(_)))
    ));
    assert!(store.bind(device(), "relative", &project.id).is_err());
    assert_eq!(store.revision(), 1);
    // Removing nothing is not a change.
    assert!(!store.unassign(device(), "s1").unwrap());
    assert_eq!(store.revision(), 1);
    store.rename_project(&project.id, "Renamed").unwrap();
    store.remove_project(&project.id).unwrap();
    assert_eq!(store.revision(), 3);
}

#[test]
fn a_device_key_that_is_not_valid_is_a_typed_error() {
    let mut store = Projects::default();
    let project = store.create_project("App", None).unwrap();
    assert!(matches!(
        store.assign("", "s1", &project.id),
        Err(crate::Error::Project(ProjectError::InvalidDevice))
    ));
}

#[gpui::test]
fn observers_are_notified_when_the_store_changes(cx: &mut TestAppContext) {
    let seen = std::rc::Rc::new(std::cell::Cell::new(0));
    cx.update(|cx| {
        Projects::default().install(cx);
        let seen = seen.clone();
        cx.observe_global::<Projects>(move |_| seen.set(seen.get() + 1))
            .detach();
    });
    cx.update(|cx| {
        Projects::update(cx, |store| store.create_project("App", None)).unwrap();
    });
    cx.run_until_parked();
    assert_eq!(seen.get(), 1);
    cx.update(|cx| assert_eq!(Projects::read(cx, |registry| registry.projects().len()), 1));
}

#[test]
fn a_bind_that_would_exceed_the_file_limit_is_refused_and_changes_nothing() {
    let mut store = Projects::default();
    let project = store.create_project("App", None).unwrap();
    let device = format!("ssh:{}", "d".repeat(100));
    let mut index = 0;
    let refused = loop {
        let path = format!("/{}/{index}", "x".repeat(900));
        index += 1;
        let before = (store.registry().clone(), store.revision());
        match store.bind(&device, &path, &project.id) {
            Ok(_) => assert!(store.registry().to_json().is_ok()),
            Err(error) => break (error, before),
        }
    };
    assert!(matches!(
        refused.0,
        crate::Error::Project(ProjectError::TooLarge)
    ));
    assert_eq!(*store.registry(), refused.1.0);
    assert_eq!(store.revision(), refused.1.1);
    // A smaller change is still accepted afterwards.
    store.rename_project(&project.id, "Renamed").unwrap();
}

/// A workspace whose saved repository remote names `owner/repo`.
fn workspace(remote: &str) -> SessionWorkspace {
    SessionWorkspace {
        project: "/work/app".to_owned(),
        path: "/work/app".to_owned(),
        repo: Some(RepoKey::from_remote_url(remote).unwrap()),
        ..Default::default()
    }
}

#[test]
fn discovery_from_an_empty_registry_persists_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let mut store = Projects::at(Some(path.clone()));
    assert!(store.registry().is_empty());
    let session = SessionRef::new(DeviceKey::new(device()).unwrap(), "work");
    let workspaces = [workspace("git@github.com:owner/app.git")];
    assert!(
        store
            .discover(std::iter::once((&session, workspaces.as_slice())))
            .unwrap()
    );
    assert_eq!(store.registry().projects().len(), 1);
    assert_eq!(store.revision(), 1);
    // The change goes through the same writer every mutation does.
    store.writer.take().unwrap().finish();
    let again = Projects::at(Some(path));
    assert_eq!(again.registry().projects().len(), 1);
    let resolution = again
        .resolve_session(device(), "work", &workspaces)
        .unwrap();
    assert!(resolution.project.is_some());
}

#[test]
fn discovery_leaves_a_manual_assignment_in_charge() {
    let mut store = Projects::default();
    let chosen = store.create_project("Chosen", None).unwrap();
    store.assign(device(), "work", &chosen.id).unwrap();
    let revision = store.revision();
    let session = SessionRef::new(DeviceKey::new(device()).unwrap(), "work");
    let workspaces = [workspace("git@github.com:owner/app.git")];
    assert!(
        !store
            .discover(std::iter::once((&session, workspaces.as_slice())))
            .unwrap()
    );
    // Nothing is created and nothing is rewritten: the user's choice wins.
    assert_eq!(store.registry().projects().len(), 1);
    assert_eq!(store.revision(), revision);
    let resolution = store
        .resolve_session(device(), "work", &workspaces)
        .unwrap();
    assert_eq!(resolution.project, Some(chosen.id));
}
