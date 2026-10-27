//! The Device and Project filters of the manager, composed with the run-state
//! filter and the search text, and how the list, page and selection restart
//! when they change.
use super::*;
use crate::{
    menu::sessions::{Entry, Row},
    projects::Projects,
};
use herdr_client::{ProjectId, RemoteSession, SessionWorkspace};

fn saved(name: &str, state: SessionState, path: Option<&str>) -> LocalSession {
    LocalSession {
        workspaces: path
            .map(|path| SessionWorkspace {
                project: path.into(),
                path: path.into(),
                ..Default::default()
            })
            .into_iter()
            .collect(),
        ..session(name, state)
    }
}

fn names(view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        view.read(cx)
            .session_entries()
            .iter()
            .filter_map(Entry::row)
            .filter_map(|row| match row {
                Row::Local(name) => Some(format!("local:{name}")),
                Row::Device { id, session } => Some(format!("{id}:{session}")),
                Row::Add(_) => None,
            })
            .collect()
    })
}

fn create(cx: &mut VisualTestContext, name: &str) -> ProjectId {
    cx.update(|_, cx| {
        Projects::update(cx, |store| store.create_project(name, None))
            .unwrap()
            .id
    })
}

fn assign(
    cx: &mut VisualTestContext,
    view: &Entity<crate::HerdrWindow>,
    name: &str,
    id: &ProjectId,
) {
    let device = cx.update(|_, cx| view.read(cx).endpoints[0].id.clone());
    cx.update(|_, cx| Projects::update(cx, |store| store.assign(&device, name, id)).unwrap());
    cx.run_until_parked();
    draw(cx);
}

/// Two local sessions in two projects plus one in none, and a remote device
/// reporting two sessions.
fn fixture(
    cx: &mut TestAppContext,
) -> (
    Entity<crate::HerdrWindow>,
    &mut VisualTestContext,
    ProjectId,
    ProjectId,
) {
    let (view, cx) = open_list(
        cx,
        vec![
            saved("alpha-run", SessionState::Running, Some("/code/a")),
            saved("alpha-stop", SessionState::Stopped, Some("/code/a2")),
            saved("beta-run", SessionState::Running, Some("/code/b")),
            saved("loose", SessionState::Running, None),
        ],
        development("default"),
    );
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.endpoints
                .push(remote("ssh:build", "Build", "build.invalid", "remote-run"));
            view.list_all_endpoints();
            view.sessions.devices.answers.insert(
                "ssh:build".into(),
                crate::sessions::DeviceScan::Sessions(vec![
                    RemoteSession {
                        name: "remote-run".into(),
                        running: true,
                        workspaces: Vec::new(),
                    },
                    RemoteSession {
                        name: "remote-stop".into(),
                        running: false,
                        workspaces: Vec::new(),
                    },
                ]),
            );
        })
    });
    let one = create(cx, "One");
    let two = create(cx, "Two");
    assign(cx, &view, "alpha-run", &one);
    assign(cx, &view, "alpha-stop", &one);
    assign(cx, &view, "beta-run", &two);
    (view, cx, one, two)
}

fn scope(
    view: &Entity<crate::HerdrWindow>,
    cx: &mut VisualTestContext,
    change: impl FnOnce(&mut crate::menu::sessions::Facets),
) {
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.change_scope(|scope| change(&mut scope.facets), cx)
        })
    });
    draw(cx);
}

#[gpui::test]
fn the_device_filter_keeps_one_device(cx: &mut TestAppContext) {
    let (view, cx, ..) = fixture(cx);
    assert_eq!(names(&view, cx).len(), 6);
    scope(&view, cx, |facets| facets.device = Some("ssh:build".into()));
    assert_eq!(
        names(&view, cx),
        ["ssh:build:remote-run", "ssh:build:remote-stop"]
    );
    let local = cx.update(|_, cx| view.read(cx).endpoints[0].id.clone());
    scope(&view, cx, |facets| facets.device = Some(local));
    let found = names(&view, cx);
    assert_eq!(found.len(), 4);
    assert!(found.iter().all(|name| name.starts_with("local:")));
}

#[gpui::test]
fn the_project_filter_keeps_one_project_and_no_project_keeps_the_rest(cx: &mut TestAppContext) {
    let (view, cx, one, _) = fixture(cx);
    scope(&view, cx, |facets| {
        facets.project = crate::menu::sessions::ProjectPick::Is(one)
    });
    let mut found = names(&view, cx);
    found.sort();
    assert_eq!(found, ["local:alpha-run", "local:alpha-stop"]);
    scope(&view, cx, |facets| {
        facets.project = crate::menu::sessions::ProjectPick::Unassigned
    });
    let mut found = names(&view, cx);
    found.sort();
    assert_eq!(
        found,
        [
            "local:loose",
            "ssh:build:remote-run",
            "ssh:build:remote-stop"
        ]
    );
}

#[gpui::test]
fn device_project_state_and_text_combine_as_and(cx: &mut TestAppContext) {
    let (view, cx, one, _) = fixture(cx);
    scope(&view, cx, |facets| {
        facets.project = crate::menu::sessions::ProjectPick::Is(one)
    });
    click(cx, "sessions-filter-Running");
    draw(cx);
    assert_eq!(names(&view, cx), ["local:alpha-run"]);
    click(cx, "sessions-filter-Stopped");
    draw(cx);
    assert_eq!(names(&view, cx), ["local:alpha-stop"]);
    // A device that holds none of the project's sessions leaves nothing.
    scope(&view, cx, |facets| facets.device = Some("ssh:build".into()));
    assert!(names(&view, cx).is_empty());
    assert!(cx.debug_bounds("sessions-no-matches").is_some());
    scope(&view, cx, |facets| facets.device = None);
    click(cx, "sessions-filter-All");
    draw(cx);
    cx.simulate_keystrokes("a 2");
    cx.run_until_parked();
    draw(cx);
    assert_eq!(names(&view, cx), ["local:alpha-stop"]);
}

#[gpui::test]
fn the_toolbar_buttons_choose_a_device_and_a_project(cx: &mut TestAppContext) {
    let (view, cx, ..) = fixture(cx);
    click(cx, "sessions-facet-Device");
    draw(cx);
    // All devices, this machine, Build.
    click(cx, "sessions-choice-Device-2");
    draw(cx);
    assert_eq!(
        names(&view, cx),
        ["ssh:build:remote-run", "ssh:build:remote-stop"]
    );
    click(cx, "sessions-facet-Device");
    draw(cx);
    click(cx, "sessions-choice-Device-0");
    draw(cx);
    assert_eq!(names(&view, cx).len(), 6);
    click(cx, "sessions-facet-Project");
    draw(cx);
    // All projects, No project, One, Two.
    click(cx, "sessions-choice-Project-1");
    draw(cx);
    assert_eq!(names(&view, cx).len(), 3);
    click(cx, "sessions-facet-Project");
    draw(cx);
    click(cx, "sessions-choice-Project-3");
    draw(cx);
    assert_eq!(names(&view, cx), ["local:beta-run"]);
}

#[gpui::test]
fn changing_a_filter_restarts_the_page_selection_and_scroll(cx: &mut TestAppContext) {
    let many: Vec<LocalSession> = (0..250)
        .map(|index| saved(&format!("s{index:03}"), SessionState::Stopped, None))
        .collect();
    let (view, cx) = open_list(cx, many, development("s000"));
    click(cx, "sessions-page-next");
    draw(cx);
    click(cx, "sessions-page-next");
    draw(cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).session_page(), 2));
    scope(&view, cx, |facets| {
        facets.project = crate::menu::sessions::ProjectPick::Unassigned
    });
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(
            view.session_page(),
            0,
            "a changed filter starts at page one"
        );
        let entries = view.session_entries();
        let children = crate::HerdrWindow::row_children(&entries);
        let selected = view.menu.selected.unwrap();
        assert_eq!(
            entries[children[selected]].row(),
            Some(&Row::Local("s000".into()))
        );
    });
    // The same filter again changes nothing, so the page the user is on stays.
    click(cx, "sessions-page-next");
    draw(cx);
    scope(&view, cx, |facets| {
        facets.project = crate::menu::sessions::ProjectPick::Unassigned
    });
    cx.update(|_, cx| assert_eq!(view.read(cx).session_page(), 1));
}

#[gpui::test]
fn a_narrow_window_keeps_every_filter_control_inside_the_panel(cx: &mut TestAppContext) {
    let (view, cx, ..) = fixture(cx);
    cx.simulate_resize(size(px(640.), px(360.)));
    cx.run_until_parked();
    draw(cx);
    click(cx, "sessions-facet-Project");
    draw(cx);
    let panel = bounds(cx, "menu-panel");
    let window = Bounds::from_corners(gpui::point(px(0.), px(0.)), gpui::point(px(640.), px(360.)));
    assert!(
        window.contains(&panel.origin)
            && window.contains(&gpui::point(panel.right(), panel.bottom()))
    );
    for selector in [
        "sessions-facet-Device",
        "sessions-facet-Project",
        "sessions-filter-All",
        "sessions-filter-Running",
        "sessions-filter-Stopped",
        "sessions-expand",
        "sessions-choice-Project-0",
        "sessions-choice-Project-3",
    ] {
        let control = bounds(cx, selector);
        assert!(control.left() >= panel.left(), "{selector} left");
        assert!(control.right() <= panel.right(), "{selector} right");
    }
    let _ = view;
}

#[gpui::test]
fn expanding_paints_a_page_sized_layout_inside_the_window(cx: &mut TestAppContext) {
    let (_view, cx, ..) = fixture(cx);
    cx.simulate_resize(size(px(1200.), px(700.)));
    cx.run_until_parked();
    draw(cx);
    let compact = bounds(cx, "menu-panel");
    click(cx, "sessions-expand");
    draw(cx);
    let page = bounds(cx, "menu-panel");
    assert!(page.size.width > compact.size.width);
    assert!(page.size.height > compact.size.height);
    assert!(page.right() <= px(1200.) && page.bottom() <= px(700.));
    // Narrow windows clamp it to the window.
    cx.simulate_resize(size(px(640.), px(360.)));
    cx.run_until_parked();
    draw(cx);
    let page = bounds(cx, "menu-panel");
    assert!(page.left() >= px(0.) && page.right() <= px(640.) && page.bottom() <= px(360.));
    assert!(bounds(cx, "sessions-expand").right() <= page.right());
}
