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
