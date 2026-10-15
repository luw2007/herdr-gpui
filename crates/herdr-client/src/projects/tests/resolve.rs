use super::*;

const APP: &str = "git@github.com:o/app.git";

fn resolved(registry: &Registry, who: &SessionRef, list: &[SessionWorkspace]) -> Option<ProjectId> {
    registry.resolve(who, list).project
}

#[test]
fn an_explicit_assignment_beats_paths_and_repositories() {
    let mut registry = Registry::new();
    let by_repo = registry.create_project("repo", Some(key(APP))).unwrap();
    let by_path = registry.create_project("path", None).unwrap();
    let chosen = registry.create_project("chosen", None).unwrap();
    registry
        .bind(device("local"), "/work/app", &by_path.id)
        .unwrap();
    let list = [workspace("/work/app", "/work/app", Some(APP))];
    let who = session("local", "main");

    let inferred = registry.resolve(&who, &list);
    assert_eq!(inferred.project.as_ref(), Some(&by_path.id));
    assert_eq!(inferred.source, Some(Source::Path("/work/app".into())));

    registry.assign(who.clone(), &chosen.id).unwrap();
    let explicit = registry.resolve(&who, &list);
    assert_eq!(explicit.project.as_ref(), Some(&chosen.id));
    assert_eq!(explicit.source, Some(Source::Assigned));
    // Assigned even with no workspaces at all.
    assert_eq!(resolved(&registry, &who, &[]), Some(chosen.id.clone()));

    registry.unbind(&device("local"), "/work/app").unwrap();
    registry.unassign(&who);
    let by_remote = registry.resolve(&who, &list);
    assert_eq!(by_remote.project.as_ref(), Some(&by_repo.id));
    assert_eq!(by_remote.source, Some(Source::Repository(key(APP))));
}

#[test]
fn the_same_repository_joins_one_project_across_devices_with_different_paths() {
    let mut registry = Registry::new();
    let app = registry.create_project("app", Some(key(APP))).unwrap();
    let mac = [workspace("/Users/me/app", "/Users/me/app", Some(APP))];
    let box_ = [workspace(
        "/srv/code/app",
        "/srv/code/app",
        Some("https://u:p@GitHub.com/O/App"),
    )];
    assert_eq!(
        resolved(&registry, &session("local", "a"), &mac),
        Some(app.id.clone())
    );
    assert_eq!(
        resolved(&registry, &session("ssh:box", "b"), &box_),
        Some(app.id)
    );
}

#[test]
fn a_non_git_project_resolves_only_after_binding_on_that_device() {
    let mut registry = Registry::new();
    let task = registry.create_project("ABC-123", None).unwrap();
    let list = [workspace("/tasks/abc", "/tasks/abc/sub", None)];
    let who = session("ssh:box", "s");
    assert_eq!(resolved(&registry, &who, &list), None);
    registry
        .bind(device("ssh:box"), "/tasks/abc", &task.id)
        .unwrap();
    assert_eq!(resolved(&registry, &who, &list), Some(task.id.clone()));
    // Another device has no such binding.
    assert_eq!(resolved(&registry, &session("local", "s"), &list), None);
}

#[test]
fn the_longest_ancestor_wins_and_matching_respects_component_boundaries() {
    let mut registry = Registry::new();
    let outer = registry.create_project("outer", None).unwrap();
    let inner = registry.create_project("inner", None).unwrap();
    let sibling = registry.create_project("sibling", None).unwrap();
    registry.bind(device("local"), "/work", &outer.id).unwrap();
    registry
        .bind(device("local"), "/work/app/deep", &inner.id)
        .unwrap();
    registry
        .bind(device("local"), "/work/app2", &sibling.id)
        .unwrap();
    let who = session("local", "s");
    let at = |path: &str| resolved(&registry, &who, &[workspace(path, path, None)]);
    assert_eq!(at("/work/app/deep/x/y"), Some(inner.id.clone()));
    assert_eq!(at("/work/app/deep"), Some(inner.id.clone()));
    assert_eq!(at("/work/app/src"), Some(outer.id.clone()));
    assert_eq!(at("/work/app2/src"), Some(sibling.id.clone()));
    assert_eq!(at("/work/app"), Some(outer.id.clone()));
    assert_eq!(at("/workspace/app"), None);
    assert_eq!(at("/other"), None);
    // Trailing and doubled separators in the workspace path do not matter.
    assert_eq!(at("/work//app/deep/"), Some(inner.id));
}

#[test]
fn a_binding_covers_a_linked_worktree_through_the_repository_root() {
    let mut registry = Registry::new();
    let task = registry.create_project("task", None).unwrap();
    registry
        .bind(device("local"), "/code/app", &task.id)
        .unwrap();
    // The worktree lives beside the repository, not below it.
    let list = [workspace("/code/app", "/code/app.wt/feature", None)];
    let found = registry.resolve(&session("local", "s"), &list);
    assert_eq!(found.project, Some(task.id));
    assert_eq!(found.source, Some(Source::Path("/code/app".into())));
}

#[test]
fn a_manual_binding_beats_the_repository_within_a_workspace_and_the_first_resolving_workspace_wins()
{
    let mut registry = Registry::new();
    let by_repo = registry.create_project("repo", Some(key(APP))).unwrap();
    let by_path = registry.create_project("path", None).unwrap();
    registry
        .bind(device("local"), "/special", &by_path.id)
        .unwrap();
    let who = session("local", "s");

    let both = [workspace("/special", "/special", Some(APP))];
    assert_eq!(resolved(&registry, &who, &both), Some(by_path.id.clone()));

    // An unresolvable workspace is skipped; order decides between resolvers.
    let skipped = [
        workspace("/nowhere", "/nowhere", None),
        workspace("/r", "/r", Some(APP)),
        workspace("/special", "/special", None),
    ];
    assert_eq!(
        resolved(&registry, &who, &skipped),
        Some(by_repo.id.clone())
    );
    let flipped = [skipped[2].clone(), skipped[1].clone()];
    assert_eq!(resolved(&registry, &who, &flipped), Some(by_path.id));
}

#[test]
fn nothing_is_guessed_from_names_labels_or_unknown_remotes() {
    let mut registry = Registry::new();
    registry.create_project("app", Some(key(APP))).unwrap();
    let mut other = workspace("/x/app", "/x/app", Some("git@github.com:o/other.git"));
    other.label = Some("app".into());
    other.branch = Some("app".into());
    assert_eq!(
        resolved(&registry, &session("local", "app"), &[other]),
        None
    );
    assert_eq!(resolved(&registry, &session("local", "main"), &[]), None);
    assert_eq!(
        registry.resolve(&session("local", "x"), &[]),
        Resolution::default()
    );
}

#[test]
fn separators_are_interchangeable_but_segments_are_exact() {
    let mut registry = Registry::new();
    let task = registry.create_project("win", None).unwrap();
    registry
        .bind(device("wsl:Ubuntu"), "C:\\Src\\App", &task.id)
        .unwrap();
    let who = session("wsl:Ubuntu", "s");
    let at = |path: &str| resolved(&registry, &who, &[workspace(path, path, None)]);
    assert_eq!(at("C:/Src/App/lib"), Some(task.id.clone()));
    assert_eq!(at("C:\\Src\\App\\lib"), Some(task.id));
    assert_eq!(at("c:/src/app/lib"), None);
    assert_eq!(at("C:/Src/Application"), None);
}
