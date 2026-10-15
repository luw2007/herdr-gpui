//! Searching, filtering and paging the session manager, driven the way a user
//! does: typing into the focused search field and clicking the toolbar.
use super::*;
use crate::menu::sessions::{Entry, Row};
use herdr_client::SessionWorkspace;

fn workspace(project: &str, path: &str, branch: Option<&str>) -> SessionWorkspace {
    SessionWorkspace {
        project: project.into(),
        path: path.into(),
        branch: branch.map(Into::into),
        label: None,
        repo: None,
    }
}

fn saved(name: &str, state: SessionState, workspaces: Vec<SessionWorkspace>) -> LocalSession {
    LocalSession {
        workspaces,
        ..session(name, state)
    }
}

/// Replace whatever the search holds with `query`, key by key.
fn type_query(cx: &mut VisualTestContext, query: &str) {
    cx.simulate_keystrokes(&vec!["backspace"; 40].join(" "));
    let keys: Vec<String> = query
        .chars()
        .map(|ch| {
            if ch == ' ' {
                "space".to_owned()
            } else {
                ch.to_string()
            }
        })
        .collect();
    if !keys.is_empty() {
        cx.simulate_keystrokes(&keys.join(" "));
    }
    cx.run_until_parked();
    draw(cx);
}

fn rows(view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext) -> Vec<Row> {
    cx.update(|_, cx| {
        view.read(cx)
            .session_entries()
            .iter()
            .filter_map(Entry::row)
            .filter(|row| matches!(row, Row::Local(_) | Row::Device { .. }))
            .cloned()
            .collect()
    })
}

fn names(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|row| match row {
            Row::Local(name) | Row::Device { session: name, .. } => name.clone(),
            Row::Add(_) => unreachable!(),
        })
        .collect()
}

fn catalog() -> Vec<LocalSession> {
    vec![
        saved("default", SessionState::Running, Vec::new()),
        // dx wt names are an irreversible hash; only the saved path says more.
        saved(
            "dx-wt-9f3a",
            SessionState::Stopped,
            vec![workspace(
                "/code/billing",
                "/code/billing.wt/feature-x",
                Some("feature/search"),
            )],
        ),
        saved(
            "dx-wt-77c1",
            SessionState::Running,
            vec![
                workspace("/code/portal", "/code/portal", Some("main")),
                workspace("/code/portal", "/code/portal.wt/fix", Some("fix/ui")),
            ],
        ),
    ]
}

#[gpui::test]
fn a_hashed_session_is_found_by_its_saved_project_path_and_branch(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    for (query, expected) in [
        ("billing", vec!["dx-wt-9f3a"]),
        ("feature/search", vec!["dx-wt-9f3a"]),
        ("portal.wt fix/ui", vec!["dx-wt-77c1"]),
        ("9f3a", vec!["dx-wt-9f3a"]),
    ] {
        type_query(cx, query);
        assert_eq!(names(&rows(&view, cx)), expected, "{query}");
    }
}

#[gpui::test]
fn no_match_paints_an_empty_result_and_nothing_to_attach(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    type_query(cx, "nothing-like-this");
    assert!(rows(&view, cx).is_empty());
    assert!(cx.debug_bounds("sessions-no-matches").is_some());
    cx.simulate_keystrokes("enter");
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.menu.page, Some(Page::Sessions));
        assert_eq!(view.endpoints[0].connection.target, development("default"));
    });
}

#[gpui::test]
fn the_state_filter_separates_running_from_stopped(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    click(cx, "sessions-filter-Stopped");
    draw(cx);
    assert_eq!(names(&rows(&view, cx)), ["dx-wt-9f3a"]);
    click(cx, "sessions-filter-Running");
    draw(cx);
    let mut running = names(&rows(&view, cx));
    running.sort();
    assert_eq!(running, ["default", "dx-wt-77c1"]);
    click(cx, "sessions-filter-All");
    draw(cx);
    assert_eq!(rows(&view, cx).len(), 3);
}

#[gpui::test]
fn sessions_group_by_project_and_unclassified_comes_last(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    // A multi-workspace session stays one row, found by any of its workspaces.
    // Project groups come first by name, unclassified sessions last.
    assert_eq!(
        names(&rows(&view, cx)),
        ["dx-wt-9f3a", "dx-wt-77c1", "default"]
    );
    type_query(cx, "fix/ui");
    assert_eq!(names(&rows(&view, cx)), ["dx-wt-77c1"]);
    let headings: Vec<String> = cx.update(|_, cx| {
        view.read(cx)
            .session_entries()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Muted { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    });
    assert!(
        headings
            .iter()
            .any(|text| text == "Unassigned · /code/portal")
    );
}

#[gpui::test]
fn same_named_sessions_on_two_devices_stay_distinct(cx: &mut TestAppContext) {
    let (view, cx) = open_list(
        cx,
        vec![session("default", SessionState::Running)],
        development("default"),
    );
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            for (id, host) in [("ssh:a", "a.invalid"), ("ssh:b", "b.invalid")] {
                view.endpoints.push(remote(id, id, host, "default"));
                view.list_all_endpoints();
                view.sessions
                    .devices
                    .answers
                    .insert(id.into(), reported(&[("shared", false)]));
            }
        })
    });
    type_query(cx, "shared stopped");
    let found = rows(&view, cx);
    assert_eq!(found.len(), 2);
    assert!(found.contains(&Row::Device {
        id: "ssh:a".into(),
        session: "shared".into()
    }));
    assert!(found.contains(&Row::Device {
        id: "ssh:b".into(),
        session: "shared".into()
    }));
}

#[gpui::test]
fn backspace_edits_the_search_and_never_asks_to_delete(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    cx.simulate_keystrokes("x y");
    assert!(rows(&view, cx).is_empty(), "the query narrowed the list");
    cx.simulate_keystrokes("backspace backspace backspace");
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(view.menu.session_edit.is_none(), "no delete confirmation");
        assert_eq!(view.menu.page, Some(Page::Sessions));
    });
    assert_eq!(rows(&view, cx).len(), 3, "the search is empty again");
    cx.simulate_keystrokes("delete");
    cx.update(|_, cx| assert!(view.read(cx).menu.session_edit.is_none()));
}

#[gpui::test]
fn a_refresh_keeps_the_selection_on_the_same_session(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    cx.simulate_keystrokes("down");
    let selected_row = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let view = view.read(cx);
            let entries = view.session_entries();
            let children = crate::HerdrWindow::row_children(&entries);
            view.menu
                .selected
                .and_then(|selected| children.get(selected))
                .and_then(|child| entries[*child].row().cloned())
        })
    };
    let before = selected_row(cx);
    assert!(before.is_some());
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            // A new session sorts ahead of the selected one.
            view.sessions.entries.insert(
                0,
                saved(
                    "aaa",
                    SessionState::Running,
                    vec![workspace("/code/a", "/code/a", None)],
                ),
            );
            view.sync_session_selection();
        })
    });
    assert_eq!(selected_row(cx), before);
}

#[gpui::test]
fn a_large_catalog_is_paged_and_every_session_is_reachable(cx: &mut TestAppContext) {
    let many: Vec<LocalSession> = (0..250)
        .map(|index| saved(&format!("s{index:03}"), SessionState::Stopped, Vec::new()))
        .collect();
    let (view, cx) = open_list(cx, many, development("s000"));
    assert_eq!(rows(&view, cx).len(), 100);
    click(cx, "sessions-page-next");
    draw(cx);
    click(cx, "sessions-page-next");
    draw(cx);
    assert_eq!(rows(&view, cx).len(), 50);
    // Searching reaches a session on any page and returns to page one.
    type_query(cx, "s249");
    assert_eq!(names(&rows(&view, cx)), ["s249"]);
}
