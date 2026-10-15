#![allow(clippy::unwrap_used)]
use super::*;
use crate::{menu::Page, sidebar::layout_tests::fixture_window};
use gpui::{TestAppContext, WindowHandle};
use herdr_client::SessionWorkspace;

fn window(cx: &mut TestAppContext) -> WindowHandle<HerdrWindow> {
    cx.add_window(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.endpoints[0].connection.target = ConnectTarget::Session {
            name: "work".into(),
            development: false,
        };
        view
    })
}

fn ask(op: ProjectOp, cx: &mut TestAppContext) -> Response {
    ask_as(Caller::default(), op, cx)
}

fn ask_as(caller: Caller, op: ProjectOp, cx: &mut TestAppContext) -> Response {
    cx.update(|cx| project(Call { caller, op }, cx))
}

fn code(response: &Response) -> Option<ErrorCode> {
    match response {
        Response::Error { code, .. } => Some(*code),
        _ => None,
    }
}

fn message(response: &Response) -> String {
    match response {
        Response::Done { message } | Response::Error { message, .. } => message.clone(),
        other => format!("{other:?}"),
    }
}

fn create(name: &str, repo: Option<&str>, cx: &mut TestAppContext) -> ProjectInfo {
    let response = ask(
        ProjectOp::Create {
            name: name.into(),
            repo: repo.map(Into::into),
        },
        cx,
    );
    let Response::Project {
        reply: Reply::Project { project },
    } = response
    else {
        panic!("not created: {response:?}");
    };
    project
}

fn listed(cx: &mut TestAppContext) -> (Vec<ProjectInfo>, usize) {
    match ask(ProjectOp::List, cx) {
        Response::Project {
            reply: Reply::Projects { projects, total },
        } => (projects, total),
        other => panic!("not a list: {other:?}"),
    }
}

fn shown(op: ProjectOp, cx: &mut TestAppContext) -> Resolved {
    match ask(op, cx) {
        Response::Project {
            reply: Reply::Resolved(resolved),
        } => *resolved,
        other => panic!("not resolved: {other:?}"),
    }
}

fn show_current() -> ProjectOp {
    ProjectOp::Show {
        session: None,
        device: None,
    }
}

#[gpui::test]
fn projects_are_created_listed_renamed_and_found_by_id_or_name(cx: &mut TestAppContext) {
    let _window = window(cx);
    assert_eq!(listed(cx), (Vec::new(), 0));
    let task = create("Task One", None, cx);
    let git = create("Alpha", Some("git@github.com:Owner/Repo.git"), cx);
    assert_eq!(task.repo, None);
    assert_eq!(git.repo.as_deref(), Some("github.com/owner/repo"));
    let (projects, total) = listed(cx);
    assert_eq!(total, 2);
    assert_eq!(
        projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Alpha", "Task One"]
    );
    let again = ask(
        ProjectOp::Create {
            name: "alpha".into(),
            repo: None,
        },
        cx,
    );
    assert_eq!(code(&again), Some(ErrorCode::Failed), "{again:?}");
    let same_repo = ask(
        ProjectOp::Create {
            name: "Other".into(),
            repo: Some("https://github.com/owner/repo".into()),
        },
        cx,
    );
    assert_eq!(code(&same_repo), Some(ErrorCode::Failed));
    let renamed = ask(
        ProjectOp::Rename {
            project: "task one".into(),
            name: "  Gamma ".into(),
        },
        cx,
    );
    assert_eq!(message(&renamed), "Renamed Task One to Gamma");
    let by_id = ask(
        ProjectOp::Rename {
            project: git.id.clone(),
            name: "Beta".into(),
        },
        cx,
    );
    assert_eq!(message(&by_id), "Renamed Alpha to Beta");
    let missing = ask(
        ProjectOp::Rename {
            project: "nothing".into(),
            name: "x".into(),
        },
        cx,
    );
    assert_eq!(code(&missing), Some(ErrorCode::NotFound));
    let blank = ask(
        ProjectOp::Rename {
            project: "beta".into(),
            name: "  ".into(),
        },
        cx,
    );
    assert_eq!(code(&blank), Some(ErrorCode::InvalidRequest), "{blank:?}");
}

#[gpui::test]
fn a_task_project_cannot_be_made_from_an_unusable_repository(cx: &mut TestAppContext) {
    let _window = window(cx);
    for repo in ["/tmp/clone", "just-a-word", "ftp://host/owner/repo"] {
        let response = ask(
            ProjectOp::Create {
                name: "x".into(),
                repo: Some(repo.into()),
            },
            cx,
        );
        assert_eq!(code(&response), Some(ErrorCode::InvalidRequest), "{repo}");
    }
    assert_eq!(listed(cx).1, 0);
}

#[test]
fn lookup_by_name_is_exact_ignores_case_and_never_guesses() {
    let mut registry = Registry::new();
    let a = registry.create_project("Alpha", None).unwrap();
    registry.create_project("Alphabet", None).unwrap();
    assert_eq!(find_project(&registry, "ALPHA").unwrap().id, a.id);
    assert_eq!(find_project(&registry, a.id.as_str()).unwrap().id, a.id);
    let unknown = find_project(&registry, "Alp").unwrap_err();
    assert_eq!(code(&unknown), Some(ErrorCode::NotFound));
    // An id that parses but names nothing falls back to names, then fails.
    let ghost = "00000000-0000-4000-8000-000000000000";
    assert_eq!(
        code(&find_project(&registry, ghost).unwrap_err()),
        Some(ErrorCode::NotFound)
    );
    let many = ambiguous(
        "project",
        "twin",
        7,
        (0..5).map(|n| format!("id{n}")),
        "use its id",
    );
    assert_eq!(code(&many), Some(ErrorCode::Ambiguous));
    let text = message(&many);
    assert!(
        text.contains("7 projects") && text.contains("..."),
        "{text}"
    );
}

#[test]
fn a_registry_with_two_equal_names_is_refused_not_guessed() {
    // Hand-edited state can hold names the API would reject; read it as is.
    let json = br#"{"version":1,"projects":[
        {"id":"11111111-1111-4111-8111-111111111111","name":"Twin"},
        {"id":"22222222-2222-4222-8222-222222222222","name":"twin"}]}"#;
    let Ok(registry) = Registry::from_json(json) else {
        // The codec refuses such a file outright, which is also acceptable.
        return;
    };
    let response = find_project(&registry, "TWIN").unwrap_err();
    assert_eq!(code(&response), Some(ErrorCode::Ambiguous));
    assert!(message(&response).contains("11111111-1111-4111-8111-111111111111"));
    assert!(find_project(&registry, "22222222-2222-4222-8222-222222222222").is_ok());
}

#[gpui::test]
fn bindings_default_to_the_windows_device_and_conflicts_are_refused(cx: &mut TestAppContext) {
    let _window = window(cx);
    let a = create("A", None, cx);
    let b = create("B", None, cx);
    let bound = ask(
        ProjectOp::Bind {
            project: "a".into(),
            path: "/work/a/".into(),
            device: None,
        },
        cx,
    );
    assert_eq!(message(&bound), "Bound /work/a on local to A");
    let again = ask(
        ProjectOp::Bind {
            project: a.id.clone(),
            path: "/work/a".into(),
            device: Some("LOCAL".into()),
        },
        cx,
    );
    assert!(!matches!(again, Response::Error { .. }), "{again:?}");
    let conflict = ask(
        ProjectOp::Bind {
            project: "B".into(),
            path: "/work/a".into(),
            device: Some("local".into()),
        },
        cx,
    );
    assert_eq!(code(&conflict), Some(ErrorCode::Failed), "{conflict:?}");
    let relative = ask(
        ProjectOp::Bind {
            project: "B".into(),
            path: "relative/dir".into(),
            device: None,
        },
        cx,
    );
    assert_eq!(code(&relative), Some(ErrorCode::InvalidRequest));
    let nowhere = ask(
        ProjectOp::Bind {
            project: "B".into(),
            path: "/work/b".into(),
            device: Some("no-such-device".into()),
        },
        cx,
    );
    assert_eq!(code(&nowhere), Some(ErrorCode::NotFound));
    let missing = ask(
        ProjectOp::Bind {
            project: "ghost".into(),
            path: "/work/b".into(),
            device: None,
        },
        cx,
    );
    assert_eq!(code(&missing), Some(ErrorCode::NotFound));
    let (projects, _) = listed(cx);
    let a_info = projects.iter().find(|p| p.id == a.id).unwrap();
    assert_eq!(
        a_info.bindings,
        [BindingInfo {
            device: "local".into(),
            path: "/work/a".into()
        }]
    );
    assert!(
        projects
            .iter()
            .find(|p| p.id == b.id)
            .unwrap()
            .bindings
            .is_empty()
    );
    let unbound = ask(
        ProjectOp::Unbind {
            path: "/work/a".into(),
            device: None,
        },
        cx,
    );
    assert_eq!(message(&unbound), "Unbound /work/a on local");
    let gone = ask(
        ProjectOp::Unbind {
            path: "/work/a".into(),
            device: None,
        },
        cx,
    );
    assert_eq!(code(&gone), Some(ErrorCode::NotFound));
    // A binding of a device that left the window can still be dropped.
    ask(
        ProjectOp::Bind {
            project: "A".into(),
            path: "/old".into(),
            device: Some("local".into()),
        },
        cx,
    );
    cx.update(|cx| {
        Projects::update(cx, |store| {
            store.bind("ssh:gone", "/srv/a", &ProjectId::parse(&a.id).unwrap())
        })
        .unwrap();
    });
    let removed = ask(
        ProjectOp::Unbind {
            path: "/srv/a".into(),
            device: Some("ssh:gone".into()),
        },
        cx,
    );
    assert_eq!(message(&removed), "Unbound /srv/a on ssh:gone");
}

#[gpui::test]
fn the_current_session_is_assigned_shown_and_unassigned(cx: &mut TestAppContext) {
    let _window = window(cx);
    create("A", None, cx);
    create("B", None, cx);
    let before = shown(show_current(), cx);
    assert_eq!(
        (before.device.as_str(), before.session.as_str()),
        ("local", "work")
    );
    assert!(before.project.is_none() && before.note.is_some());
    let assigned = ask(
        ProjectOp::Assign {
            project: "a".into(),
            session: None,
            device: None,
        },
        cx,
    );
    assert_eq!(message(&assigned), "Assigned session work on local to A");
    let now = shown(show_current(), cx);
    assert_eq!(now.project.as_ref().unwrap().name, "A");
    assert_eq!(now.source, Some(SourceInfo::Assigned));
    assert!(now.note.is_none());
    // One project per session: assigning again moves it.
    ask(
        ProjectOp::Assign {
            project: "B".into(),
            session: Some("work".into()),
            device: Some("local".into()),
        },
        cx,
    );
    assert_eq!(shown(show_current(), cx).project.unwrap().name, "B");
    let counts: Vec<_> = listed(cx)
        .0
        .iter()
        .map(|p| (p.name.clone(), p.sessions))
        .collect();
    assert_eq!(counts, [("A".into(), 0), ("B".into(), 1)]);
    let other = shown(
        ProjectOp::Show {
            session: Some("elsewhere".into()),
            device: None,
        },
        cx,
    );
    assert!(other.project.is_none());
    let removed = ask(
        ProjectOp::Unassign {
            session: None,
            device: None,
        },
        cx,
    );
    assert_eq!(message(&removed), "Unassigned session work on local");
    let none = ask(
        ProjectOp::Unassign {
            session: None,
            device: None,
        },
        cx,
    );
    assert!(message(&none).contains("no explicit assignment"));
    assert!(shown(show_current(), cx).project.is_none());
    let unknown_project = ask(
        ProjectOp::Assign {
            project: "ghost".into(),
            session: None,
            device: None,
        },
        cx,
    );
    assert_eq!(code(&unknown_project), Some(ErrorCode::NotFound));
}

#[gpui::test]
fn show_explains_a_folder_and_a_repository_from_the_scanned_workspaces(cx: &mut TestAppContext) {
    let handle = window(cx);
    let task = create("Task", None, cx);
    let repo = create("Repo", Some("github.com/o/r"), cx);
    ask(
        ProjectOp::Bind {
            project: task.id.clone(),
            path: "/work/task".into(),
            device: None,
        },
        cx,
    );
    let workspace = |path: &str, repo: Option<&str>| SessionWorkspace {
        project: path.into(),
        path: path.into(),
        branch: None,
        label: None,
        repo: repo.and_then(herdr_client::RepoKey::from_remote_url),
    };
    let scanned = |cx: &mut TestAppContext, workspaces: Vec<SessionWorkspace>| {
        handle
            .update(cx, |view, _, _| {
                view.sessions.entries = vec![herdr_client::LocalSession {
                    name: "work".into(),
                    state: herdr_client::SessionState::Stopped,
                    socket: "/tmp/none.sock".into(),
                    workspaces,
                }];
            })
            .unwrap();
    };
    scanned(cx, vec![workspace("/work/task/src", None)]);
    let by_path = shown(show_current(), cx);
    assert_eq!(by_path.project.unwrap().id, task.id);
    assert_eq!(
        by_path.source,
        Some(SourceInfo::Path {
            path: "/work/task".into()
        })
    );
    scanned(
        cx,
        vec![workspace("/other", Some("https://github.com/o/r.git"))],
    );
    let by_repo = shown(show_current(), cx);
    assert_eq!(by_repo.project.unwrap().id, repo.id);
    assert_eq!(
        by_repo.source,
        Some(SourceInfo::Repository {
            repo: "github.com/o/r".into()
        })
    );
    scanned(cx, vec![workspace("/nowhere", None)]);
    let none = shown(show_current(), cx);
    assert!(none.project.is_none());
    assert!(none.note.unwrap().contains("no folder or repository"));
}

#[gpui::test]
fn a_session_the_window_cannot_name_must_be_named(cx: &mut TestAppContext) {
    let handle = window(cx);
    create("A", None, cx);
    handle
        .update(cx, |view, _, _| {
            view.endpoints[0].connection.target = ConnectTarget::Socket("/tmp/x.sock".into());
        })
        .unwrap();
    for op in [
        show_current(),
        ProjectOp::Assign {
            project: "A".into(),
            session: None,
            device: None,
        },
        ProjectOp::Unassign {
            session: None,
            device: None,
        },
    ] {
        let response = ask(op, cx);
        assert_eq!(
            code(&response),
            Some(ErrorCode::InvalidRequest),
            "{response:?}"
        );
        assert!(message(&response).contains("--session"));
    }
    let named = ask(
        ProjectOp::Assign {
            project: "A".into(),
            session: Some("chosen".into()),
            device: None,
        },
        cx,
    );
    assert!(!matches!(named, Response::Error { .. }), "{named:?}");
}

#[gpui::test]
fn requests_are_refused_rather_than_acted_in_another_sessions_window(cx: &mut TestAppContext) {
    let _window = window(cx);
    create("A", None, cx);
    let elsewhere = || Caller {
        workspace_id: Some("w_missing".into()),
        daemon_socket: Some("/tmp/other-session.sock".into()),
        ..Caller::default()
    };
    let named_gone = || Caller {
        window: Some(4242),
        ..Caller::default()
    };
    for op in [
        ProjectOp::List,
        ProjectOp::Create {
            name: "B".into(),
            repo: None,
        },
        ProjectOp::Rename {
            project: "A".into(),
            name: "C".into(),
        },
        ProjectOp::Assign {
            project: "A".into(),
            session: Some("s".into()),
            device: None,
        },
        ProjectOp::Show {
            session: Some("s".into()),
            device: None,
        },
        ProjectOp::Delete {
            project: "A".into(),
        },
    ] {
        let refused = ask_as(elsewhere(), op.clone(), cx);
        assert_eq!(code(&refused), Some(ErrorCode::WorkspaceNotFound), "{op:?}");
        let gone = ask_as(named_gone(), op.clone(), cx);
        assert_eq!(code(&gone), Some(ErrorCode::NotFound), "{op:?}");
    }
    // Nothing above changed the registry.
    let (projects, total) = listed(cx);
    assert_eq!((total, projects[0].name.as_str()), (1, "A"));
    assert_eq!(projects[0].sessions, 0);
}

#[gpui::test]
fn a_caller_without_a_window_gets_no_window(cx: &mut TestAppContext) {
    for op in [
        ProjectOp::List,
        ProjectOp::Delete {
            project: "A".into(),
        },
    ] {
        assert_eq!(code(&ask(op, cx)), Some(ErrorCode::NoWindow));
    }
}

#[gpui::test]
fn malformed_requests_are_refused_before_anything_is_looked_up(cx: &mut TestAppContext) {
    for op in [
        ProjectOp::Create {
            name: "x\u{1b}".into(),
            repo: None,
        },
        ProjectOp::Show {
            session: Some("a/b".into()),
            device: None,
        },
        ProjectOp::Unbind {
            path: String::new(),
            device: None,
        },
    ] {
        assert_eq!(code(&ask(op, cx)), Some(ErrorCode::InvalidRequest));
    }
}

#[gpui::test]
fn delete_opens_the_confirmation_and_deletes_nothing(cx: &mut TestAppContext) {
    let handle = window(cx);
    let doomed = create("Doomed", None, cx);
    ask(
        ProjectOp::Assign {
            project: "Doomed".into(),
            session: Some("work".into()),
            device: None,
        },
        cx,
    );
    let response = ask(
        ProjectOp::Delete {
            project: "doomed".into(),
        },
        cx,
    );
    let Response::Project {
        reply: Reply::DeletePending { project },
    } = response
    else {
        panic!("not pending: {response:?}");
    };
    assert_eq!(project.id, doomed.id);
    let (open, target) = handle
        .update(cx, |view, _, _| {
            (
                view.menu.page == Some(Page::Sessions),
                view.project_removal_pending().map(|id| id.to_string()),
            )
        })
        .unwrap();
    assert!(open);
    assert_eq!(target.as_deref(), Some(doomed.id.as_str()));
    // Still there, with its assignment, until the user confirms.
    let (projects, _) = listed(cx);
    assert_eq!((projects.len(), projects[0].sessions), (1, 1));
    // A second request while the dialog holds the window is refused.
    let busy = ask(
        ProjectOp::Delete {
            project: "doomed".into(),
        },
        cx,
    );
    assert_eq!(code(&busy), Some(ErrorCode::WindowBusy));
    // Confirming in the window deletes it.
    handle
        .update(cx, |view, window, cx| {
            view.confirm_project_form(window, cx);
        })
        .unwrap();
    assert_eq!(listed(cx).1, 0);
}

#[gpui::test]
fn delete_of_an_unknown_project_opens_nothing(cx: &mut TestAppContext) {
    let handle = window(cx);
    let response = ask(
        ProjectOp::Delete {
            project: "ghost".into(),
        },
        cx,
    );
    assert_eq!(code(&response), Some(ErrorCode::NotFound));
    let open = handle
        .update(cx, |view, _, _| view.menu.page.is_some())
        .unwrap();
    assert!(!open);
}

#[test]
fn store_failures_map_to_codes_a_caller_can_act_on() {
    use crate::Error;
    let unknown = Error::Project(ProjectError::UnknownProject(
        ProjectId::parse("00000000-0000-4000-8000-000000000000").unwrap(),
    ));
    assert_eq!(code(&refuse(&unknown)), Some(ErrorCode::NotFound));
    assert_eq!(
        code(&refuse(&Error::Project(ProjectError::InvalidPath))),
        Some(ErrorCode::InvalidRequest)
    );
    assert_eq!(
        code(&refuse(&Error::Project(ProjectError::DuplicateName(
            "x".into()
        )))),
        Some(ErrorCode::Failed)
    );
}

#[test]
fn a_list_is_cut_to_its_bounds_and_says_how_many_there_are() {
    let mut registry = Registry::new();
    let project = registry.create_project("Busy", None).unwrap();
    for n in 0..(MAX_LISTED_BINDINGS + 4) {
        registry
            .bind(
                DeviceKey::new("local").unwrap(),
                &format!("/p/{n}"),
                &project.id,
            )
            .unwrap();
    }
    let item = info(&registry, &project);
    assert_eq!(item.bindings.len(), MAX_LISTED_BINDINGS);
    assert_eq!(item.bindings_total, MAX_LISTED_BINDINGS + 4);
}
