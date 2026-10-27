//! The visible entry points into project management: the toolbar's
//! **New project…** button with no project and no session, and the per-row
//! **Project…** button that opens the picker for an existing session. Here

use super::projects::{catalog, create, layout, local_id, repo};
use super::*;
use crate::menu::sessions::{Entry, Row};
use crate::projects::Projects;
use gpui::{ScrollDelta, ScrollWheelEvent, point};

fn project_names(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        Projects::read(cx, |store| {
            store
                .projects()
                .into_iter()
                .map(|project| project.name.clone())
                .collect()
        })
    })
}

/// The `sessions-project-N` selector of a local session's **Project…** button.
/// The suffix is the row's ordinal among the selectable entries — what the
/// painter receives — not the entry's index in the full line list.
fn project_button(
    view: &Entity<crate::HerdrWindow>,
    cx: &mut VisualTestContext,
    name: &str,
) -> String {
    cx.update(|_, cx| {
        let view = view.read(cx);
        let entries = view.session_entries();
        crate::HerdrWindow::row_children(&entries)
            .into_iter()
            .position(|child| {
                entries
                    .get(child)
                    .and_then(Entry::row)
                    .is_some_and(|row| matches!(row, Row::Local(found) if found == name))
            })
            .map(|ordinal| format!("sessions-project-{ordinal}"))
            .unwrap_or_else(|| panic!("{name} is not painted"))
    })
}

/// Clicks `selector` after scrolling it into the sessions list's viewport. The
/// **Project…** button of a row low in a short popup is painted below the
/// visible clip; a real user scrolls to it first, and a click outside the clip
/// reaches nothing.
fn click_in_list(cx: &mut VisualTestContext, selector: String) {
    // `debug_bounds` wants a `'static` selector; the existing test modules
    // accept a leaked `String` for that, so follow them for one row lookup.
    let leaked: &'static str = selector.clone().leak();
    let viewport = bounds(cx, "sessions-list");
    for _ in 0..40 {
        let button = bounds(cx, leaked);
        if button.top() >= viewport.top() && button.bottom() <= viewport.bottom() {
            cx.simulate_click(button.center(), Modifiers::default());
            return;
        }
        let delta = if button.center().y > viewport.center().y {
            -px(120.)
        } else {
            px(120.)
        };
        cx.simulate_event(ScrollWheelEvent {
            position: viewport.center(),
            delta: ScrollDelta::Pixels(point(px(0.), delta)),
            ..Default::default()
        });
        draw(cx);
    }
    panic!("{selector} never scrolled into the list viewport");
}

#[gpui::test]
fn the_toolbar_creates_a_project_with_no_session_and_no_prior_project(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    assert!(
        project_names(cx).is_empty(),
        "the fixture starts with no project"
    );
    assert!(
        cx.update(|_, cx| view
            .read(cx)
            .session_entries()
            .iter()
            .any(|entry| matches!(entry, Entry::Muted { selector, .. }
                if selector == "sessions-no-projects"))),
        "the empty state points at the entries"
    );
    draw(cx);
    click(cx, "sessions-new-project");
    draw(cx);
    assert!(
        cx.debug_bounds("session-modal").is_some(),
        "the create dialog opens"
    );
    cx.simulate_keystrokes("s o l o");
    click(cx, "session-edit-submit");
    cx.run_until_parked();
    draw(cx);
    assert_eq!(project_names(cx), ["solo".to_owned()]);
    assert!(
        cx.debug_bounds("project-device-0").is_some(),
        "a successful standalone create continues to the server chooser"
    );
}

#[gpui::test]
fn a_row_project_button_creates_and_assigns_in_one_pass(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    draw(cx);
    let selector = project_button(&view, cx, "d-bare");
    click_in_list(cx, selector);
    draw(cx);
    click(cx, "project-new");
    draw(cx);
    cx.simulate_keystrokes("p a i r e d");
    click(cx, "session-edit-submit");
    cx.run_until_parked();
    draw(cx);
    assert_eq!(project_names(cx), ["paired".to_owned()]);
    assert_eq!(layout(&view, cx)[0], "# paired");
    assert_eq!(layout(&view, cx)[1], "d-bare");
}

#[gpui::test]
fn the_row_picker_assigns_an_existing_project_and_restores_automatic_membership(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    let device = local_id(&view, cx);
    create(cx, "Chosen", Some(repo()));
    draw(cx);
    let selector = project_button(&view, cx, "d-bare");
    click_in_list(cx, selector);
    draw(cx);
    click(cx, "project-choice-0");
    cx.run_until_parked();
    draw(cx);
    // Chosen is a repository project, so the explicit assignment puts d-bare
    // under the same heading as the clone the repository infers.
    let assigned = layout(&view, cx);
    let heading = assigned
        .iter()
        .position(|line| line == "# Chosen")
        .unwrap_or_else(|| panic!("Chosen groups the sessions: {assigned:?}"));
    assert!(
        assigned[heading + 1..]
            .iter()
            .take_while(|line| !line.starts_with("# "))
            .any(|line| line == "d-bare"),
        "the picker assigns d-bare to Chosen: {assigned:?}"
    );
    // Dropping the explicit assignment leaves d-bare to the repository
    // inference, which knows nothing about it: the row returns to Unassigned
    // while b-repo still resolves to Chosen through its clone.
    cx.update(|_, cx| {
        Projects::update(cx, |store| store.unassign(&device, "d-bare")).unwrap();
    });
    cx.run_until_parked();
    draw(cx);
    let restored = layout(&view, cx);
    let chosen = restored
        .iter()
        .position(|line| line == "# Chosen")
        .unwrap_or_else(|| panic!("Chosen still groups b-repo: {restored:?}"));
    assert!(
        restored[chosen + 1..]
            .iter()
            .take_while(|line| !line.starts_with("# "))
            .all(|line| line != "d-bare"),
        "without an assignment d-bare leaves Chosen: {restored:?}"
    );
    let unassigned = restored
        .iter()
        .position(|line| line == "# Unassigned")
        .unwrap_or_else(|| panic!("d-bare falls back to Unassigned: {restored:?}"));
    assert_eq!(
        restored[unassigned + 1],
        "d-bare",
        "the unassigned heading holds d-bare: {restored:?}"
    );
}

#[gpui::test]
fn the_project_facet_lists_a_created_project(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    create(cx, "Facet", None);
    draw(cx);
    click(cx, "sessions-facet-Project");
    draw(cx);
    // All projects, No project, Facet.
    click(cx, "sessions-choice-Project-2");
    cx.run_until_parked();
    draw(cx);
    let rows = cx.update(|_, cx| {
        view.read(cx)
            .session_entries()
            .iter()
            .filter_map(Entry::row)
            .cloned()
            .collect::<Vec<_>>()
    });
    assert!(rows.is_empty(), "no session belongs to Facet yet: {rows:?}");
}

#[gpui::test]
fn the_no_projects_note_leaves_once_a_project_exists(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    let note = |view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            view.read(cx).session_entries().iter().any(|entry| {
                matches!(entry, Entry::Muted { selector, .. }
                    if selector == "sessions-no-projects")
            })
        })
    };
    assert!(note(&view, cx));
    create(cx, "First", None);
    cx.run_until_parked();
    draw(cx);
    assert!(!note(&view, cx));
}
