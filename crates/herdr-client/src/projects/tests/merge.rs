use super::*;

const APP: &str = "git@github.com:o/app.git";

fn discovered(registry: &mut Registry, device: &str, path: &str) -> ProjectId {
    let session = SessionRef::new(DeviceKey::new(device).unwrap(), "s");
    let list = [SessionWorkspace {
        project: path.into(),
        path: path.into(),
        ..Default::default()
    }];
    registry.discover(&session, &list).unwrap();
    registry.resolve(&session, &list).project.unwrap()
}

fn repo_project(registry: &mut Registry) -> ProjectId {
    let session = SessionRef::new(DeviceKey::new("local").unwrap(), "s");
    let list = [SessionWorkspace {
        project: "/x".into(),
        path: "/x".into(),
        repo: Some(RepoKey::from_remote_url(APP).unwrap()),
        ..Default::default()
    }];
    registry.discover(&session, &list).unwrap();
    registry.resolve(&session, &list).project.unwrap()
}

#[test]
fn concurrent_discovery_of_the_same_identity_merges_into_one() {
    let base = Registry::new();
    let mut mine = base.clone();
    let mut theirs = base.clone();
    let mine_id = discovered(&mut mine, "local", "/work/app");
    let theirs_id = discovered(&mut theirs, "local", "/work/app");
    assert_eq!(mine_id, theirs_id);
    let merged = Registry::merge(&base, &mine, &theirs).unwrap();
    assert_eq!(merged.projects().len(), 1);
    assert_eq!(merged.inferred_paths_of(&mine_id).len(), 1);
    assert_eq!(Registry::merge(&base, &theirs, &mine).unwrap(), merged);
}

#[test]
fn disjoint_inferred_discoveries_are_both_kept() {
    let base = Registry::new();
    let mut mine = base.clone();
    let mut theirs = base.clone();
    let a = discovered(&mut mine, "local", "/work/a");
    let b = discovered(&mut theirs, "local", "/work/b");
    assert_ne!(a, b);
    let merged = Registry::merge(&base, &mine, &theirs).unwrap();
    assert_eq!(merged.projects().len(), 2);
    assert_eq!(merged.inferred_paths_of(&a).len(), 1);
    assert_eq!(merged.inferred_paths_of(&b).len(), 1);
}

#[test]
fn removing_a_project_drops_its_inferred_paths() {
    let mut registry = Registry::new();
    let id = discovered(&mut registry, "local", "/work/app");
    assert_eq!(registry.inferred_paths_of(&id).len(), 1);
    registry.remove_project(&id).unwrap();
    assert!(registry.inferred_paths_of(&id).is_empty());
    assert!(registry.is_empty());
}

#[test]
fn forgetting_a_device_clears_its_inferred_paths() {
    let mut registry = Registry::new();
    let local = discovered(&mut registry, "local", "/work/app");
    let boxed = discovered(&mut registry, "ssh:box", "/srv/app");
    assert_eq!(registry.inferred_paths_of(&local).len(), 1);
    assert_eq!(registry.inferred_paths_of(&boxed).len(), 1);
    assert!(registry.forget_device(&DeviceKey::new("local").unwrap()));
    assert!(registry.inferred_paths_of(&local).is_empty());
    assert_eq!(registry.inferred_paths_of(&boxed).len(), 1);
    assert!(!registry.forget_device(&DeviceKey::new("local").unwrap()));
}

#[test]
fn a_repository_discovery_merges_to_one_project_across_windows() {
    let base = Registry::new();
    let mut mine = base.clone();
    let mut theirs = base.clone();
    let mine_id = repo_project(&mut mine);
    let theirs_id = repo_project(&mut theirs);
    assert_eq!(mine_id, theirs_id);
    let merged = Registry::merge(&base, &mine, &theirs).unwrap();
    assert_eq!(merged.projects().len(), 1);
    assert_eq!(
        merged
            .project(&mine_id)
            .unwrap()
            .repo
            .as_ref()
            .unwrap()
            .as_str(),
        "github.com/o/app"
    );
}

#[test]
fn concurrent_same_name_projects_are_rejected_without_changing_either_registry() {
    let base = Registry::new();
    let mut mine = base.clone();
    let mut theirs = base.clone();
    let first = mine.create_project("Shared", None).unwrap();
    let second = theirs.create_project("SHARED", None).unwrap();
    assert_ne!(first.id, second.id);
    for result in [
        Registry::merge(&base, &mine, &theirs),
        Registry::merge(&base, &theirs, &mine),
    ] {
        assert!(matches!(result, Err(ProjectError::DuplicateName(_))));
    }
    assert_eq!(Registry::from_json(&mine.to_json().unwrap()).unwrap(), mine);
    assert_eq!(
        Registry::from_json(&theirs.to_json().unwrap()).unwrap(),
        theirs
    );
}

#[test]
fn concurrent_project_additions_cannot_exceed_the_registry_limit() {
    let mut base = Registry::new();
    for n in 0..MAX_PROJECTS - 1 {
        base.create_project(&format!("Project {n}"), None).unwrap();
    }
    let mut mine = base.clone();
    let mut theirs = base.clone();
    mine.create_project("Mine", None).unwrap();
    theirs.create_project("Theirs", None).unwrap();
    assert!(matches!(
        Registry::merge(&base, &mine, &theirs),
        Err(ProjectError::LimitReached(Limit::Projects))
    ));
}
