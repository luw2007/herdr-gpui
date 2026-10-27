#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn shown_ids_round_trip_in_native_window_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shown.json");
    let (first, second) = (WindowId::from(1), WindowId::from(2));
    let (one, two) = (ContainerId::new(), ContainerId::new());
    let mut store = Store::new(
        Vec::new(),
        Some(Writer::start("window-shown", path.clone()).unwrap()),
    );
    assert!(store.record(first, one));
    assert!(store.record(second, two));
    assert!(!store.record(second, two));
    store.save(&[first, second]);
    store.writer.take().unwrap().finish();
    assert_eq!(read(&path).unwrap(), vec![Some(one), Some(two)]);
}

#[test]
fn restored_ids_are_handed_to_windows_in_order() {
    let (one, three) = (ContainerId::new(), ContainerId::new());
    let mut store = Store::new(vec![Some(one), None, Some(three)], None);
    store.advance();
    assert_eq!(store.take(), Some(one));
    store.advance();
    assert_eq!(store.take(), None);
    store.advance();
    assert_eq!(store.take(), Some(three));
    store.advance();
    assert_eq!(store.take(), None);
}

#[test]
fn a_closed_window_is_forgotten_and_unknown_windows_save_null() {
    let mut store = Store::default();
    let (a, b) = (WindowId::from(1), WindowId::from(2));
    store.record(a, ContainerId::new());
    store.forget(a);
    assert_eq!(store.snapshot(&[a, b]), vec![None, None]);
}

#[test]
fn invalid_or_excessive_files_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shown.json");
    std::fs::write(&path, b"broken").unwrap();
    assert!(read(&path).is_err());
    std::fs::write(&path, b"[\"not-a-uuid\"]").unwrap();
    assert!(read(&path).is_err());
    let many = vec![None::<ContainerId>; MAX_WINDOWS + 1];
    std::fs::write(&path, serde_json::to_vec(&many).unwrap()).unwrap();
    assert!(matches!(read(&path), Err(Error::InvalidWindowState)));
}
