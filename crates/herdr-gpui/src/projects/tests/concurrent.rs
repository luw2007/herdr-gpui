use super::*;
use std::time::{Duration, Instant};

fn await_project(path: &Path, name: &str) -> Registry {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(registry) = Registry::from_json(&bytes)
            && registry
                .projects()
                .iter()
                .any(|project| project.name == name)
        {
            return registry;
        }
        assert!(
            Instant::now() < deadline,
            "projects writer never saved {name}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn independent_projects_stores_alternate_without_losing_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let mut first = Projects::at(Some(path.clone()));
    let mut second = Projects::at(Some(path.clone()));
    let one = first.create_project("First", None).unwrap();
    await_project(&path, "First");
    let two = second.create_project("Second", None).unwrap();
    let disk = await_project(&path, "Second");
    assert!(disk.project(&one.id).is_some());
    first.create_project("Third", None).unwrap();
    let disk = await_project(&path, "Third");
    assert!(disk.project(&one.id).is_some());
    assert!(disk.project(&two.id).is_some());
    first.writer.unwrap().finish();
    second.writer.unwrap().finish();
}

#[test]
fn a_project_file_damaged_after_loading_is_preserved_before_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projects.json");
    let mut store = Projects::at(Some(path.clone()));
    let before = store.create_project("Before", None).unwrap();
    await_project(&path, "Before");
    std::fs::write(&path, b"{damaged while open").unwrap();
    let after = store.create_project("After", None).unwrap();
    store.writer.unwrap().finish();
    let disk = Registry::from_json(&std::fs::read(&path).unwrap()).unwrap();
    assert!(disk.project(&before.id).is_some());
    assert!(disk.project(&after.id).is_some());
    assert_eq!(
        std::fs::read(dir.path().join("projects.json.bad")).unwrap(),
        b"{damaged while open"
    );
}
