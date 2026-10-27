#![allow(clippy::unwrap_used)]

// Not `use super::*`: its `gpui::*` glob would shadow the built-in `#[test]`
// that `#[gpui::test]` expands to.
use crate::{
    HerdrWindow,
    containers::{ContainerId, Grouping},
    sidebar::layout_tests::fixture_window,
};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, px, size};

mod session_create;
mod session_entry;

/// A 640x360 window listing two Windows: the first holds the fixture's
/// session, the second is empty. Returns the view and both ids.
fn two_windows(
    cx: &mut TestAppContext,
) -> (
    Entity<HerdrWindow>,
    &mut VisualTestContext,
    ContainerId,
    ContainerId,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(640.), px(360.)));
    let (full, empty) = view.update(cx, |view, cx| {
        let full = view.containers.containers()[0].id;
        let empty = view.new_container(Some("Second"), cx).unwrap();
        (full, empty)
    });
    draw(cx);
    (view, cx, full, empty)
}

/// `debug_bounds` wants a `'static` selector; a test may leak the few it builds.
fn bounds_of(cx: &mut VisualTestContext, selector: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn right_click_tab(cx: &mut VisualTestContext, id: ContainerId) {
    let tab = bounds_of(cx, &format!("container-tab-{id}")).unwrap();
    cx.simulate_mouse_down(tab.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(tab.center(), MouseButton::Right, Modifiers::default());
    draw(cx);
}

fn click(cx: &mut VisualTestContext, selector: &str) {
    let bounds = bounds_of(cx, selector).unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

fn title_of(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, id: ContainerId) -> String {
    view.read_with(cx, |view, _| {
        view.containers.set().get(id).unwrap().title.clone()
    })
}

fn menu_open(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.container_menu.is_some())
}

fn type_title(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, title: &str) {
    view.update(cx, |view, cx| {
        let input = view.container_menu.as_ref().unwrap().input.clone();
        input.update(cx, |input, cx| input.set_text_selected(title, cx));
    });
    draw(cx);
}

#[gpui::test]
fn right_click_on_a_tab_opens_its_menu(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    assert!(bounds_of(cx, "container-menu-rename").is_none());
    right_click_tab(cx, empty);
    assert!(menu_open(&view, cx));
    for row in [
        "container-menu-rename",
        "container-menu-close",
        "container-menu-group-none",
        "container-menu-group-project",
        "container-menu-group-status",
        "container-menu-group-mr",
    ] {
        assert!(bounds_of(cx, row).is_some(), "{row}");
    }
}

#[gpui::test]
fn the_ellipsis_button_of_the_selected_tab_opens_the_menu(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    // The new Window is shown, so its tab carries the button.
    click(cx, &format!("container-menu-{empty}"));
    assert!(menu_open(&view, cx));
    assert!(bounds_of(cx, "container-menu-rename").is_some());
}

#[gpui::test]
fn renaming_updates_the_title_and_closes_the_menu(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-rename");
    type_title(&view, cx, "Renamed");
    click(cx, "container-menu-rename-submit");
    assert_eq!(title_of(&view, cx, empty), "Renamed");
    assert!(!menu_open(&view, cx));
}

#[gpui::test]
fn renaming_to_a_duplicate_title_shows_the_error_and_stays_open(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_windows(cx);
    let taken = title_of(&view, cx, full);
    right_click_tab(cx, empty);
    click(cx, "container-menu-rename");
    type_title(&view, cx, &taken);
    click(cx, "container-menu-rename-submit");
    assert!(menu_open(&view, cx));
    assert!(bounds_of(cx, "container-menu-error").is_some());
    assert_eq!(title_of(&view, cx, empty), "Second");
}

#[gpui::test]
fn cancelling_a_rename_changes_nothing(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-rename");
    type_title(&view, cx, "Other");
    click(cx, "container-menu-rename-cancel");
    assert_eq!(title_of(&view, cx, empty), "Second");
    assert!(!menu_open(&view, cx));
}

#[gpui::test]
fn close_is_refused_for_a_window_with_sessions_and_works_for_an_empty_one(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_windows(cx);
    right_click_tab(cx, full);
    click(cx, "container-menu-close");
    view.read_with(cx, |view, _| {
        assert!(
            view.containers
                .set()
                .get(full)
                .is_some_and(|c| !c.members.is_empty())
        );
        assert!(view.containers.set().get(full).is_some());
        assert_eq!(view.containers.containers().len(), 2);
    });
    assert!(menu_open(&view, cx));
    // The strip's × follows the same rule: it closes nothing and shows the menu.
    view.update(cx, |view, cx| {
        view.container_menu = None;
        cx.notify();
    });
    draw(cx);
    click(cx, &format!("container-close-{full}"));
    view.read_with(cx, |view, _| {
        assert!(view.containers.set().get(full).is_some());
    });
    assert!(menu_open(&view, cx));

    view.update(cx, |view, cx| {
        view.container_menu = None;
        cx.notify();
    });
    draw(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-close");
    view.read_with(cx, |view, _| {
        assert!(view.containers.set().get(empty).is_none());
        assert!(view.containers.set().get(full).is_some());
    });
    assert!(!menu_open(&view, cx));
}

#[gpui::test]
fn enter_on_the_refused_close_row_shows_why(cx: &mut TestAppContext) {
    let (view, cx, full, _) = two_windows(cx);
    right_click_tab(cx, full);
    assert!(bounds_of(cx, "container-menu-error").is_none());
    // Pick the Close row directly instead of stepping past the rows that grew
    // around it, so this test does not bind to the menu's row order.
    view.update(cx, |view, cx| {
        let index = super::ROWS
            .iter()
            .position(|row| *row == super::Row::Close)
            .unwrap();
        view.container_menu.as_mut().unwrap().highlight = index;
        cx.notify();
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(menu_open(&view, cx));
    assert!(bounds_of(cx, "container-menu-error").is_some());
    view.read_with(cx, |view, _| {
        let error = view.container_menu.as_ref().unwrap().error.clone().unwrap();
        assert!(error.contains("Detach its sessions first"), "{error}");
        assert!(view.containers.set().get(full).is_some());
    });
}

#[gpui::test]
fn grouping_rows_set_the_grouping_and_mark_the_current_one(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    for (grouping, selector) in [
        (Grouping::Project, "container-menu-group-project"),
        (Grouping::Status, "container-menu-group-status"),
        (Grouping::Mr, "container-menu-group-mr"),
        (Grouping::None, "container-menu-group-none"),
    ] {
        right_click_tab(cx, empty);
        click(cx, selector);
        view.read_with(cx, |view, _| {
            assert_eq!(view.containers.set().get(empty).unwrap().grouping, grouping);
        });
        assert!(!menu_open(&view, cx));
        right_click_tab(cx, empty);
        assert!(bounds_of(cx, &format!("{selector}-check")).is_some());
        for other in ["none", "project", "status", "mr"] {
            let check = format!("container-menu-group-{other}-check");
            assert_eq!(
                bounds_of(cx, &check).is_some(),
                selector.ends_with(other),
                "{check}"
            );
        }
        view.update(cx, |view, cx| {
            view.container_menu = None;
            cx.notify();
        });
        // Without a redraw the old overlay still occludes the tab.
        draw(cx);
    }
}

#[gpui::test]
fn escape_closes_the_menu_and_a_closed_window_drops_it(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    right_click_tab(cx, empty);
    cx.simulate_keystrokes("escape");
    assert!(!menu_open(&view, cx));

    right_click_tab(cx, empty);
    view.update(cx, |view, cx| {
        view.close_container(empty, cx).unwrap();
    });
    draw(cx);
    draw(cx);
    assert!(bounds_of(cx, "container-menu-rename").is_none());
    assert!(!menu_open(&view, cx));
}

#[gpui::test]
fn the_menu_and_the_strip_stay_inside_a_small_window(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    let viewport = gpui::Bounds::new(gpui::point(px(0.), px(0.)), size(px(640.), px(360.)));
    let strip = bounds_of(cx, "container-strip").unwrap();
    assert!(viewport.contains(&strip.origin) && strip.right() <= viewport.right());
    let tab = bounds_of(cx, &format!("container-tab-{empty}")).unwrap();
    assert!(tab.right() <= strip.right() && tab.bottom() <= strip.bottom());

    // Opened at the far bottom-right corner the panel must snap back inside.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let corner = gpui::point(px(639.), px(359.));
            view.open_container_menu(empty, corner, window, cx);
        })
    });
    draw(cx);
    let selectors = [
        "container-menu-panel",
        "container-menu-create-session",
        "container-menu-add-session",
        "container-menu-rename",
        "container-menu-close",
        "container-menu-group-none",
        "container-menu-group-project",
        "container-menu-group-status",
        "container-menu-group-mr",
    ];
    for selector in selectors {
        let bounds = bounds_of(cx, selector).unwrap();
        assert!(bounds.left() >= viewport.left(), "{selector}");
        assert!(bounds.top() >= viewport.top(), "{selector}");
        assert!(bounds.right() <= viewport.right(), "{selector}");
        assert!(bounds.bottom() <= viewport.bottom(), "{selector}");
    }
    // The strip is still laid out whole behind the overlay.
    assert_eq!(bounds_of(cx, "container-strip").unwrap(), strip);
}
