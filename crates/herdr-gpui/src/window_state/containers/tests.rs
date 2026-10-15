#![allow(clippy::unwrap_used)]
use super::*;
use crate::containers::SessionRef;

fn set(session: &str) -> ContainerSet {
    ContainerSet::new(SessionRef::new("local", session))
}

#[test]
fn sets_round_trip_in_native_window_order_and_stay_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("containers.json");
    let (first, second) = (WindowId::from(1), WindowId::from(2));
    let mut store = Store::new(
        Vec::new(),
        Some(Writer::start("containers", path.clone()).unwrap()),
    );
    assert!(store.record(first, &set("one")));
    assert!(store.record(second, &set("two")));
    assert!(!store.record(second, &set("two")));
    store.save(&[first, second]);
    store.writer.take().unwrap().finish();
    let read = read(&path).unwrap();
    assert_eq!(read, vec![Some(set("one")), Some(set("two"))]);
}

#[test]
fn restored_sets_are_handed_to_windows_in_order() {
    let mut store = Store::new(vec![Some(set("one")), None, Some(set("three"))], None);
    store.advance();
    assert_eq!(store.take(), Some(set("one")));
    store.advance();
    assert_eq!(store.take(), None);
    store.advance();
    assert_eq!(store.take(), Some(set("three")));
    store.advance();
    assert_eq!(store.take(), None);
}

#[test]
fn a_closed_window_is_forgotten_and_unknown_windows_save_null() {
    let mut store = Store::default();
    let (a, b) = (WindowId::from(1), WindowId::from(2));
    store.record(a, &set("a"));
    store.forget(a);
    assert_eq!(store.snapshot(&[a, b]), vec![None, None]);
}

#[test]
fn invalid_or_excessive_files_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("containers.json");
    std::fs::write(&path, b"broken").unwrap();
    assert!(read(&path).is_err());
    let mut bad = set("x");
    bad.attach(0, SessionRef::new("", "y")).ok();
    let json = serde_json::to_vec(&vec![Some(set("ok")), Some(bad)]).unwrap();
    std::fs::write(&path, json).unwrap();
    assert!(matches!(read(&path), Err(Error::Container(_))));
    let many = vec![None::<ContainerSet>; MAX_WINDOWS + 1];
    std::fs::write(&path, serde_json::to_vec(&many).unwrap()).unwrap();
    assert!(matches!(read(&path), Err(Error::Container(_))));
    assert!(read(&dir.path().join("absent.json")).unwrap().is_empty());
}
