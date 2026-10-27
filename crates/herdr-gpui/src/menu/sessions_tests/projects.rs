//! The manager groups by resolved project, finds sessions by project name and
//! keeps its selection on a session's identity when a project changes.
use super::*;
use crate::{
    menu::sessions::{Entry, Row},
    projects::Projects,
};
use herdr_client::{ProjectId, RepoKey, SessionWorkspace};

fn workspace(path: &str, repo: Option<&RepoKey>) -> SessionWorkspace {
    SessionWorkspace {
        project: path.into(),
        path: path.into(),
        branch: None,
        label: None,
        repo: repo.cloned(),
        ..Default::default()
    }
}

fn saved(name: &str, workspaces: Vec<SessionWorkspace>) -> LocalSession {
    LocalSession {
        workspaces,
        ..session(name, SessionState::Stopped)
    }
}

pub(super) fn repo() -> RepoKey {
    RepoKey::from_remote_url("https://github.com/acme/app.git").unwrap()
}

fn type_query(cx: &mut VisualTestContext, query: &str) {
    cx.simulate_keystrokes(&vec!["backspace"; 20].join(" "));
    let keys: Vec<String> = query.chars().map(|ch| ch.to_string()).collect();
    cx.simulate_keystrokes(&keys.join(" "));
    cx.run_until_parked();
    draw(cx);
}

/// Headings and rows in paint order.
pub(super) fn layout(view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        view.read(cx)
            .session_entries()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Muted { selector, text } if selector.starts_with("sessions-project-") => {
                    Some(format!("# {text}"))
                }
                Entry::Row {
                    row: Row::Local(name),
                    ..
                } => Some(name.clone()),
                _ => None,
            })
            .collect()
    })
}

pub(super) fn local_id(view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| view.read(cx).endpoints[0].id.clone())
}

pub(super) fn catalog() -> Vec<LocalSession> {
    vec![
        saved("a-task", vec![workspace("/tasks/alpha/src", None)]),
        saved("b-repo", vec![workspace("/code/app.wt/x", Some(&repo()))]),
        saved("c-loose", vec![workspace("/code/loose", None)]),
        saved("d-bare", Vec::new()),
    ]
}

pub(super) fn create(cx: &mut VisualTestContext, name: &str, repo: Option<RepoKey>) -> ProjectId {
    cx.update(|_, cx| {
        Projects::update(cx, |store| store.create_project(name, repo))
            .unwrap()
            .id
    })
}

#[gpui::test]
fn sessions_group_under_assigned_inferred_and_bound_projects(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    let device = local_id(&view, cx);
    let app = create(cx, "App", Some(repo()));
    let alpha = create(cx, "Alpha", None);
    cx.update(|_, cx| {
        Projects::update(cx, |store| store.bind(&device, "/tasks/alpha", &alpha)).unwrap();
        Projects::update(cx, |store| store.assign(&device, "c-loose", &app)).unwrap();
    });
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        layout(&view, cx),
        [
            "# Alpha",
            "a-task", // by bound folder
            "# App",
            "b-repo",
            "c-loose", // inferred, then explicitly assigned
            "# Unassigned",
            "d-bare",
        ]
    );
    // The deciding source is a subtle detail, not part of the row identity.
    cx.update(|_, cx| {
        let view = view.read(cx);
        let detail = view
            .session_entries()
            .iter()
            .find_map(|entry| match entry {
                Entry::Row {
                    row: Row::Local(name),
                    spec,
                } if name == "a-task" => Some(spec.detail.to_string()),
                _ => None,
            })
            .unwrap();
        assert!(detail.contains("by folder"), "{detail}");
    });
}

#[gpui::test]
fn unassigned_sessions_keep_their_path_label(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    let layout = layout(&view, cx);
    assert!(
        layout.contains(&"# Unassigned · /code/loose".to_owned()),
        "{layout:?}"
    );
}

#[gpui::test]
fn search_matches_the_resolved_project_name(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    let app = create(cx, "Zebra", Some(repo()));
    let device = local_id(&view, cx);
    cx.update(|_, cx| {
        Projects::update(cx, |store| store.assign(&device, "d-bare", &app)).unwrap();
    });
    type_query(cx, "zebra");
    assert_eq!(layout(&view, cx), ["# Zebra", "b-repo", "d-bare"]);
}

#[gpui::test]
fn selection_follows_the_session_across_a_reassignment_and_a_scan(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    let device = local_id(&view, cx);
    let alpha = create(cx, "Alpha", None);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.pin_session(Some(Row::Local("d-bare".into())));
            view.sync_session_selection();
            cx.notify();
        })
    });
    // The pinned session moves to the top group while a scan adds another.
    cx.update(|_, cx| {
        Projects::update(cx, |store| store.assign(&device, "d-bare", &alpha)).unwrap();
        view.update(cx, |view, _| {
            view.sessions
                .entries
                .push(saved("e-new", vec![workspace("/new", None)]));
            view.sync_session_selection();
        });
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let view = view.read(cx);
        let entries = view.session_entries();
        let children = crate::HerdrWindow::row_children(&entries);
        let selected = view.menu.selected.map(|index| &entries[children[index]]);
        assert_eq!(
            selected.and_then(Entry::row),
            Some(&Row::Local("d-bare".into()))
        );
    });
}

#[gpui::test]
fn another_window_change_notifies_this_one(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    let before = cx.update(|_, cx| view.read(cx).sessions.projects.projects().len());
    create(cx, "Late", None);
    cx.run_until_parked();
    let after = cx.update(|_, cx| view.read(cx).sessions.projects.projects().len());
    assert_eq!((before, after), (0, 1));
}

#[gpui::test]
fn moving_a_session_through_the_modal_assigns_it(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    create(cx, "Alpha", None);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_project_move(Row::Local("d-bare".into()), window, cx)
        })
    });
    draw(cx);
    click(cx, "project-choice-0");
    cx.run_until_parked();
    draw(cx);
    cx.update(|_, cx| assert!(view.read(cx).menu.session_edit.is_none()));
    assert_eq!(layout(&view, cx)[0], "# Alpha");
    assert_eq!(layout(&view, cx)[1], "d-bare");
}
