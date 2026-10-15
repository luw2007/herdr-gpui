use super::*;

fn repo(name: &str) -> RepoKey {
    key(&format!("git@github.com:o/{name}.git"))
}

#[test]
fn names_are_trimmed_bounded_and_unique_ignoring_case() {
    let mut registry = Registry::new();
    let alpha = registry.create_project("  Alpha ", None).unwrap();
    assert_eq!(alpha.name, "Alpha");
    assert!(matches!(
        registry.create_project("ALPHA", None),
        Err(ProjectError::DuplicateName(_))
    ));
    for bad in ["", "   ", "a\nb", &"x".repeat(65)] {
        assert!(matches!(
            registry.create_project(bad, None),
            Err(ProjectError::InvalidName)
        ));
    }
    registry.create_project(&"x".repeat(64), None).unwrap();
    let beta = registry.create_project("beta", None).unwrap();
    assert!(matches!(
        registry.rename_project(&beta.id, "alpha"),
        Err(ProjectError::DuplicateName(_))
    ));
    // Renaming to a different case of its own name is allowed.
    registry.rename_project(&alpha.id, "ALPHA").unwrap();
    assert_eq!(registry.project(&alpha.id).unwrap().name, "ALPHA");
    assert!(matches!(
        registry.rename_project(&ProjectId::generate(), "gamma"),
        Err(ProjectError::UnknownProject(_))
    ));
}

#[test]
fn one_project_per_repository() {
    let mut registry = Registry::new();
    let first = registry.create_project("app", Some(repo("app"))).unwrap();
    match registry.create_project("app two", Some(key("https://github.com/O/App"))) {
        Err(ProjectError::DuplicateRepo { existing }) => assert_eq!(existing, first.id),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        registry.project_for_repo(&repo("app")).unwrap().id,
        first.id
    );
    assert_eq!(registry.projects().len(), 1);
}

#[test]
fn projects_are_listed_by_name_case_insensitively() {
    let mut registry = Registry::new();
    for name in ["beta", "Alpha", "gamma"] {
        registry.create_project(name, None).unwrap();
    }
    let names: Vec<&str> = registry
        .projects()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["Alpha", "beta", "gamma"]);
}

#[test]
fn bindings_validate_paths_and_conflict_across_projects() {
    let mut registry = Registry::new();
    let a = registry.create_project("a", None).unwrap();
    let b = registry.create_project("b", None).unwrap();
    let bound = registry.bind(device("local"), "/work/a///", &a.id).unwrap();
    assert_eq!(bound.path, "/work/a");
    // Same project again is a no-op; another project conflicts.
    registry.bind(device("local"), "/work/a", &a.id).unwrap();
    assert!(matches!(
        registry.bind(device("local"), "/work/a/", &b.id),
        Err(ProjectError::BindingConflict { existing, .. }) if existing == a.id
    ));
    // The same path on another device is a different binding.
    registry.bind(device("ssh:dev"), "/work/a", &b.id).unwrap();
    for bad in [
        "",
        "relative/dir",
        "~/a",
        "/a\u{0}b",
        &format!("/{}", "x".repeat(1024)),
    ] {
        assert!(
            matches!(
                registry.bind(device("local"), bad, &a.id),
                Err(ProjectError::InvalidPath)
            ),
            "{bad:?}"
        );
    }
    assert_eq!(
        registry
            .bind(device("wsl:Ubuntu"), "C:\\src\\a\\", &a.id)
            .unwrap()
            .path,
        "C:\\src\\a"
    );
    assert_eq!(
        registry.bind(device("local"), "/", &a.id).unwrap().path,
        "/"
    );
    assert_eq!(
        registry.bind(device("local"), "D:\\", &a.id).unwrap().path,
        "D:\\"
    );
    assert!(registry.bind(device("local"), "\\\\", &a.id).is_err());
    assert!(matches!(
        registry.bind(device("local"), "/x", &ProjectId::generate()),
        Err(ProjectError::UnknownProject(_))
    ));
    assert!(registry.unbind(&device("local"), "/work/a/").unwrap());
    assert!(!registry.unbind(&device("local"), "/work/a").unwrap());
    // Once unbound, the folder can go to the other project.
    registry.bind(device("local"), "/work/a", &b.id).unwrap();
    let paths: Vec<_> = registry
        .bindings_of(&b.id)
        .into_iter()
        .map(|x| (x.device.to_string(), x.path))
        .collect();
    assert_eq!(
        paths,
        [
            ("local".into(), "/work/a".into()),
            ("ssh:dev".into(), "/work/a".into())
        ]
    );
}

#[test]
fn assignment_replaces_and_rejects_bad_sessions_and_projects() {
    let mut registry = Registry::new();
    let a = registry.create_project("a", None).unwrap();
    let b = registry.create_project("b", None).unwrap();
    let s = session("local", "main");
    registry.assign(s.clone(), &a.id).unwrap();
    registry.assign(s.clone(), &b.id).unwrap();
    assert_eq!(registry.assignment(&s), Some(&b.id));
    assert!(registry.assignments_of(&a.id).is_empty());
    assert_eq!(registry.assignments_of(&b.id), [&s]);
    assert!(matches!(
        registry.assign(session("local", "../x"), &a.id),
        Err(ProjectError::InvalidSession)
    ));
    assert!(matches!(
        registry.assign(s.clone(), &ProjectId::generate()),
        Err(ProjectError::UnknownProject(_))
    ));
    // The failed attempt left the earlier assignment alone.
    assert_eq!(registry.assignment(&s), Some(&b.id));
    assert!(registry.unassign(&s));
    assert!(!registry.unassign(&s));
    assert_eq!(registry.assignment(&s), None);
}

#[test]
fn removing_a_project_drops_its_bindings_and_assignments_only() {
    let mut registry = Registry::new();
    let a = registry.create_project("a", Some(repo("a"))).unwrap();
    let b = registry.create_project("b", None).unwrap();
    registry.bind(device("local"), "/a", &a.id).unwrap();
    registry.bind(device("local"), "/b", &b.id).unwrap();
    registry.assign(session("local", "one"), &a.id).unwrap();
    registry.assign(session("local", "two"), &b.id).unwrap();
    assert_eq!(registry.remove_project(&a.id).unwrap().name, "a");
    assert_eq!(registry.project(&a.id), None);
    assert!(registry.bindings_of(&a.id).is_empty());
    assert_eq!(registry.assignment(&session("local", "one")), None);
    assert_eq!(registry.assignment(&session("local", "two")), Some(&b.id));
    assert_eq!(registry.bindings_of(&b.id).len(), 1);
    // Its repository and name are free again.
    registry.create_project("a", Some(repo("a"))).unwrap();
    assert!(matches!(
        registry.remove_project(&a.id),
        Err(ProjectError::UnknownProject(_))
    ));
}

#[test]
fn collection_limits_are_enforced() {
    let mut registry = Registry::new();
    let first = registry.create_project("p0", None).unwrap();
    for index in 1..MAX_PROJECTS {
        registry.create_project(&format!("p{index}"), None).unwrap();
    }
    assert!(matches!(
        registry.create_project("one more", None),
        Err(ProjectError::LimitReached(Limit::Projects))
    ));
    for index in 0..MAX_ASSIGNMENTS {
        registry
            .assign(session("local", &format!("s{index}")), &first.id)
            .unwrap();
    }
    assert!(matches!(
        registry.assign(session("local", "extra"), &first.id),
        Err(ProjectError::LimitReached(Limit::Assignments))
    ));
    // Re-assigning an existing session does not need a free slot.
    registry.assign(session("local", "s0"), &first.id).unwrap();
    for index in 0..MAX_BINDINGS {
        registry
            .bind(device("local"), &format!("/p/{index}"), &first.id)
            .unwrap();
    }
    assert!(matches!(
        registry.bind(device("local"), "/p/extra", &first.id),
        Err(ProjectError::LimitReached(Limit::Bindings))
    ));
}

#[test]
fn device_keys_are_bounded_and_control_free() {
    assert!(DeviceKey::new("").is_err());
    assert!(DeviceKey::new("a\tb").is_err());
    assert!(DeviceKey::new("x".repeat(129)).is_err());
    assert!(DeviceKey::new("ssh:my-host").is_ok());
    assert!(DeviceKey::new("x".repeat(128)).is_ok());
}

#[test]
fn project_ids_round_trip_only_in_canonical_form() {
    let id = ProjectId::generate();
    assert_eq!(ProjectId::parse(id.as_str()).unwrap(), id);
    assert!(ProjectId::parse(&id.as_str().to_uppercase()).is_err());
    assert!(ProjectId::parse(&id.as_str().replace('-', "")).is_err());
    assert!(ProjectId::parse("not-an-id").is_err());
}
