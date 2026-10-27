use super::*;

const APP: &str = "git@github.com:o/app.git";
const TOOL: &str = "https://gitlab.com/g/s/tool";

fn discover(registry: &mut Registry, who: &SessionRef, list: &[SessionWorkspace]) -> bool {
    registry.discover(who, list).unwrap()
}

#[test]
fn an_empty_registry_joins_repository_sessions_across_devices_and_paths() {
    let mut registry = Registry::new();
    let laptop = [workspace("/home/u/app", "/home/u/app", Some(APP))];
    let desktop = [workspace(
        "C:\\src\\app",
        "C:\\src\\app",
        Some("https://github.com/O/App"),
    )];
    let first = session("local", "main");
    let second = session("ssh:box", "work");

    assert!(discover(&mut registry, &first, &laptop));
    // The second clone is already placed once the repository project exists.
    assert!(!discover(&mut registry, &second, &desktop));

    // One project for both clones, and no bindings were needed.
    assert_eq!(registry.projects().len(), 1);
    let project = registry.projects()[0];
    assert_eq!(project.repo.as_ref(), Some(&key(APP)));
    assert!(project.name.starts_with("app · "));
    assert!(registry.bindings_of(&project.id).is_empty());
    for (who, list) in [(&first, &laptop[..]), (&second, &desktop[..])] {
        let resolution = registry.resolve(who, list);
        assert_eq!(resolution.project.as_ref(), Some(&project.id));
        assert_eq!(resolution.source, Some(Source::Repository(key(APP))));
    }
    // The same identity discovered independently in another empty registry
    // yields the same id and name, so the two merge instead of colliding.
    let mut other = Registry::new();
    assert!(discover(&mut other, &first, &laptop));
    assert_eq!(other.projects()[0].id, project.id);
    assert_eq!(other.projects()[0].name, project.name);
}

#[test]
fn repo_less_fallback_roots_group_and_round_trip() {
    let mut registry = Registry::new();
    let who = session("local", "task");
    // No remote: the saved common root is the identity, not the path.
    let list = [workspace("/work/tasks/ABC", "/work/tasks/ABC/sub", None)];

    assert!(discover(&mut registry, &who, &list));
    let project = registry.projects()[0].clone();
    assert_eq!(project.repo, None);
    assert!(project.name.starts_with("ABC · "));
    assert_eq!(
        registry.inferred_paths_of(&project.id),
        [Binding {
            device: device("local"),
            path: "/work/tasks/ABC".into(),
            project: project.id.clone(),
        }]
    );
    assert!(registry.bindings_of(&project.id).is_empty());
    let resolution = registry.resolve(&who, &list);
    assert_eq!(resolution.project.as_ref(), Some(&project.id));
    assert_eq!(
        resolution.source,
        Some(Source::Path("/work/tasks/ABC".into()))
    );

    // The inference survives a save/load round trip as its own list.
    let bytes = registry.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["version"], 2);
    assert_eq!(value["inferred_paths"][0]["path"], "/work/tasks/ABC");
    assert_eq!(value["bindings"].as_array().unwrap().len(), 0);
    let mut loaded = Registry::from_json(&bytes).unwrap();
    assert_eq!(loaded, registry);
    assert_eq!(
        loaded.resolve(&who, &list).project.as_ref(),
        Some(&project.id)
    );
    // Discovery is idempotent, and a sibling under the inferred root joins the
    // same project via that existing inference.
    assert!(!discover(&mut loaded, &who, &list));
    let sibling = session("local", "task-2");
    let other = [workspace(
        "/work/tasks/ABC/other",
        "/work/tasks/ABC/other",
        None,
    )];
    assert!(!discover(&mut loaded, &sibling, &other));
    assert_eq!(loaded.projects().len(), 1);
    assert_eq!(
        loaded.resolve(&sibling, &other).project.as_ref(),
        Some(&project.id)
    );
    assert_eq!(loaded.to_json().unwrap(), registry.to_json().unwrap());
}

#[test]
fn manual_bindings_and_assignments_win_and_are_never_rewritten() {
    let mut registry = Registry::new();
    let manual = registry.create_project("mine", None).unwrap();
    registry.bind(device("local"), "/work", &manual.id).unwrap();
    let who = session("local", "main");
    let list = [workspace("/work/app", "/work/app", Some(APP))];

    // A covering manual binding resolves the session, so discover does nothing:
    // no repository project is invented beside the user's choice.
    assert!(!discover(&mut registry, &who, &list));
    assert_eq!(registry.projects().len(), 1);
    assert_eq!(
        registry.resolve(&who, &list).project.as_ref(),
        Some(&manual.id)
    );

    // An assignment does the same even with no binding at all.
    let assigned = registry.create_project("assigned", None).unwrap();
    let other = session("ssh:box", "main");
    let remote = [workspace("/srv/app", "/srv/app", Some(APP))];
    registry.assign(other.clone(), &assigned.id).unwrap();
    assert!(!discover(&mut registry, &other, &remote));
    assert_eq!(registry.projects().len(), 2);
    assert_eq!(registry.assignment(&other), Some(&assigned.id));
    assert!(registry.bindings_of(&assigned.id).is_empty());
}

#[test]
fn a_manual_binding_shadows_an_inferred_one_without_conflict() {
    let mut registry = Registry::new();
    let who = session("local", "main");
    let list = [workspace("/work/app", "/work/app", None)];
    assert!(discover(&mut registry, &who, &list));
    let inferred = registry.projects()[0].id.clone();

    // The user binds the same folder to a project of their own: that is a
    // manual binding, so it needs no unbind and simply wins.
    let mine = registry.create_project("mine", None).unwrap();
    registry
        .bind(device("local"), "/work/app", &mine.id)
        .unwrap();
    assert_eq!(
        registry.resolve(&who, &list).project.as_ref(),
        Some(&mine.id)
    );
    // Discovery respects the manual binding, leaving the inferred project alone.
    assert!(!discover(&mut registry, &who, &list));
    assert_eq!(registry.projects().len(), 2);
    assert_eq!(registry.inferred_paths_of(&inferred).len(), 1);
    assert!(registry.bindings_of(&inferred).is_empty());
    assert_eq!(registry.bindings_of(&mine.id).len(), 1);
    // Unbinding the manual entry uncovers the inference again.
    assert!(registry.unbind(&device("local"), "/work/app").unwrap());
    assert_eq!(
        registry.resolve(&who, &list).project.as_ref(),
        Some(&inferred)
    );
}

#[test]
fn colliding_labels_still_produce_distinct_names() {
    let mut registry = Registry::new();
    // Pre-build the label discovery would pick for the second repo, and claim
    // it as a user project on a different repo — forcing a longer suffix.
    let first_repo = "https://gitlab.com/g/s/tool";
    let second_repo = "https://github.com/h/d/tool";
    let short_label = {
        let mut probe = Registry::new();
        probe
            .discover(
                &session("local", "p"),
                &[workspace("/s", "/s", Some(second_repo))],
            )
            .unwrap();
        probe.projects()[0].name.clone()
    };
    let user = registry
        .create_project(&short_label, Some(key(first_repo)))
        .unwrap();
    let who = session("local", "two");
    assert!(discover(
        &mut registry,
        &who,
        &[workspace("/b", "/b", Some(second_repo))]
    ));
    let all = registry.projects();
    let discovered = all.iter().find(|p| p.id != user.id).unwrap();
    assert!(discovered.name.starts_with("tool · "));
    assert_ne!(discovered.name, short_label);
    assert!(discovered.name.chars().count() <= 64);
    // Every label is unique case-insensitively.
    let mut names: Vec<String> = registry
        .projects()
        .iter()
        .map(|p| p.name.to_lowercase())
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(registry.projects().len(), names.len());
}

#[test]
fn blank_or_unusable_paths_are_ignored_instead_of_guessed() {
    let mut registry = Registry::new();
    let who = session("local", "main");
    for list in [
        vec![workspace("", "", None)],
        vec![workspace("   ", "   ", None)],
        vec![workspace("relative", "relative", None)],
        vec![workspace("~/app", "~/app", None)],
        vec![workspace("/", "/", None)],
        vec![workspace("D:\\", "D:\\", None)],
        vec![workspace("\\\\", "\\\\", None)],
        vec![workspace("\\\\server", "\\\\server", None)],
        vec![workspace("/a\u{0}b", "/a\u{0}b", None)],
        vec![workspace("/work/../app", "/work/../app", None)],
        vec![workspace("/work/./app", "/work/./app", None)],
        {
            let p = format!("/{}", "x".repeat(1024));
            vec![workspace(p.as_str(), p.as_str(), None)]
        },
    ] {
        assert!(!discover(&mut registry, &who, &list), "{list:?}");
        assert!(registry.is_empty(), "{list:?}");
    }
    // A later usable workspace in the same session still counts.
    let mixed = [
        workspace("", "", None),
        workspace("/work/ABC", "/work/ABC", None),
    ];
    assert!(discover(&mut registry, &who, &mixed));
    assert_eq!(registry.projects().len(), 1);
    assert!(registry.resolve(&who, &mixed).project.is_some());
}

#[test]
fn a_failed_batch_leaves_the_registry_untouched() {
    let mut registry = Registry::new();
    let filler = registry.create_project("filler", None).unwrap();
    for index in 0..MAX_BINDINGS {
        registry
            .bind(device("local"), &format!("/p/{index}"), &filler.id)
            .unwrap();
    }
    let before = registry.clone();
    let first = session("local", "a");
    let second = session("local", "b");
    let list = [workspace("/work/app", "/work/app", None)];
    // The first session fits, the second hits the shared binding cap: the
    // whole call fails and nothing from it is kept.
    let error = registry
        .discover_sessions([(&first, &list[..]), (&second, &list[..])])
        .unwrap_err();
    assert!(matches!(error, ProjectError::LimitReached(Limit::Bindings)));
    assert_eq!(registry, before);
}

#[test]
fn deleting_a_discovered_project_is_recreated_by_the_next_scan() {
    let mut registry = Registry::new();
    let who = session("local", "main");
    let list = [workspace("/work/ABC", "/work/ABC", None)];
    assert!(discover(&mut registry, &who, &list));
    let first = registry.projects()[0].clone();
    assert!(
        registry
            .remove_project(&first.id)
            .unwrap()
            .name
            .starts_with("ABC · ")
    );
    // Its inference went with it, so the session resolves to nothing again.
    assert!(registry.resolve(&who, &list).project.is_none());
    assert!(registry.is_empty());
    assert!(discover(&mut registry, &who, &list));
    let again = registry.projects()[0].clone();
    assert_eq!(again.id, first.id);
    assert_eq!(again.name, first.name);
}

#[test]
fn unused_repository_projects_are_not_created_for_bound_workspaces() {
    let mut registry = Registry::new();
    let manual = registry.create_project("mine", None).unwrap();
    registry
        .bind(device("ssh:box"), "/srv", &manual.id)
        .unwrap();
    let who = session("ssh:box", "main");
    // The repository is present, but a manual binding already decides.
    let list = [workspace("/srv/app", "/srv/app", Some(TOOL))];
    assert!(!discover(&mut registry, &who, &list));
    assert_eq!(registry.projects().len(), 1);
    assert!(registry.project_for_repo(&key(TOOL)).is_none());
}

#[test]
fn a_discovered_name_fits_the_limit_and_keeps_the_repository_tail() {
    let mut registry = Registry::new();
    let long = "https://example.com/owner/a-very-long-repository-name-that-goes-on-and-on";
    let who = session("local", "main");
    let list = [workspace("/x", "/x", Some(long))];
    assert!(discover(&mut registry, &who, &list));
    let project = registry.projects()[0];
    assert!(project.name.chars().count() <= 64, "{}", project.name);
    assert!(project.name.starts_with("a-very-long-repository-name"));
    assert!(project.name.contains(" · "));
    // The same tail from another host is another repository, another project.
    let other = session("local", "other");
    let elsewhere = [workspace(
        "/y",
        "/y",
        Some("https://gitlab.com/x/a-very-long-repository-name-that-goes-on-and-on"),
    )];
    assert!(discover(&mut registry, &other, &elsewhere));
    assert_eq!(registry.projects().len(), 2);
    let names: Vec<&str> = registry
        .projects()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_ne!(names[0], names[1]);
}

#[test]
fn counts_are_bounded_and_discovered_identities_stay_stable() {
    let mut registry = Registry::new();
    for index in 0..MAX_PROJECTS {
        registry.create_project(&format!("p{index}"), None).unwrap();
    }
    let who = session("local", "main");
    let list = [workspace("/work/app", "/work/app", None)];
    assert!(matches!(
        registry.discover(&who, &list),
        Err(ProjectError::LimitReached(Limit::Projects))
    ));
    // Discovery derives ids from the identity alone, so every window agrees.
    let mut one = Registry::new();
    let mut two = Registry::new();
    discover(&mut one, &who, &list);
    discover(&mut two, &who, &list);
    assert_eq!(one, two);
    assert_eq!(one.projects().len(), 1);
}
