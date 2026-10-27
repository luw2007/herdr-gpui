#![allow(clippy::unwrap_used)]

//! Discoverability of adding or creating a session to an existing Window.
//!
//! These cover the surfaces added so a populated single Window still offers a
//! way to add to itself: the fixed strip entry, the two rows the Window's tab
//! menu gained, and the handoff between that menu and the Sessions picker.

use crate::{
    HerdrWindow,
    containers::{ContainerId, ContainerView},
    menu::Page,
    sidebar::layout_tests::fixture_window,
};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, px, size};
use herdr_client::{ConnectTarget, LocalSession, SessionState};

/// Two Windows with a consistent local session identity. Creating the second
/// Window shows it, so switch back before exercising a non-active tab.
fn two_windows_development(
    cx: &mut TestAppContext,
) -> (
    Entity<HerdrWindow>,
    &mut VisualTestContext,
    ContainerId,
    ContainerId,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.endpoints[0].connection.target = development_target("local");
        view.containers = ContainerView::local(&view.endpoints[0].session_ref());
        view.sessions
            .entries
            .push(session("keep", SessionState::Running));
        view
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    let (full, empty) = view.update(cx, |view, cx| {
        let full = view.containers.containers()[0].id;
        let empty = view.new_container(Some("Second"), cx).unwrap();
        assert!(view.switch_container(full, cx));
        (full, empty)
    });
    draw(cx);
    (view, cx, full, empty)
}

fn development_target(name: &str) -> ConnectTarget {
    ConnectTarget::Session {
        name: name.into(),
        development: true,
    }
}

/// A saved local session whose socket path follows the same rules the window
/// resolves, so no test hard-codes a home directory.
fn session(name: &str, state: SessionState) -> LocalSession {
    LocalSession {
        name: name.into(),
        state,
        socket: development_target(name).socket_path().unwrap(),
        workspaces: Vec::new(),
    }
}

fn bounds_of(
    cx: &mut VisualTestContext,
    selector: &'static str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    cx.debug_bounds(selector)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn right_click_tab(cx: &mut VisualTestContext, id: ContainerId) {
    // gpui's test `debug_bounds` takes a `&'static str`; the sibling tab-menu
    // tests leak the few tab selectors they build, so follow that bound pattern
    // for the one id-derived selector this module needs.
    let selector: &'static str = Box::leak(format!("container-tab-{id}").into_boxed_str());
    let tab = bounds_of(cx, selector).unwrap();
    cx.simulate_mouse_down(tab.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(tab.center(), MouseButton::Right, Modifiers::default());
    draw(cx);
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = bounds_of(cx, selector).unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

fn menu_open(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.container_menu.is_some())
}

fn picker_open(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.menu.page == Some(Page::Sessions))
}
fn active_id(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> ContainerId {
    view.read_with(cx, |view, _| view.containers.active().id)
}

/// A populated single Window keeps the strip visible, and its fixed Add-session
/// entry still sits inside the strip at a narrow width.
#[gpui::test]
fn the_strip_and_its_add_entry_remain_visible_for_a_lone_populated_window(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(640.), px(360.)));
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.containers.containers().len(), 1);
        assert!(!view.containers.active().members.is_empty());
        assert!(view.container_strip_shown(), "the strip is always on");
    });
    let strip = bounds_of(cx, "container-strip").unwrap();
    let add = bounds_of(cx, "container-strip-add-session").unwrap();
    assert!(add.left() >= strip.left());
    assert!(add.right() <= strip.right());
    assert!(add.top() >= strip.top());
    assert!(add.bottom() <= strip.bottom());
}

/// The strip entry opens the picker for the current Window.
#[gpui::test]
fn the_strip_add_entry_opens_the_sessions_picker(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(640.), px(360.)));
    draw(cx);
    click(cx, "container-strip-add-session");
    assert!(picker_open(&view, cx));
    assert!(!menu_open(&view, cx));
}

/// Create is refused before the form opens on a socket target: pressing Enter
/// on the row shows the reason instead of opening the picker.
#[gpui::test]
fn create_session_is_refused_before_the_form_opens(cx: &mut TestAppContext) {
    let (view, cx, full, _) = two_windows_development(cx);
    view.update(cx, |view, _| {
        view.endpoints[0].connection.target = ConnectTarget::Socket("/tmp/unused.sock".into());
    });
    draw(cx);
    right_click_tab(cx, full);
    assert!(menu_open(&view, cx));
    view.update(cx, |view, cx| {
        let index = crate::window::container_menu::ROWS
            .iter()
            .position(|row| *row == crate::window::container_menu::Row::CreateSession)
            .unwrap();
        view.container_menu.as_mut().unwrap().highlight = index;
        cx.notify();
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(menu_open(&view, cx));
    assert!(!picker_open(&view, cx));
    view.read_with(cx, |view, _| {
        let error = view.container_menu.as_ref().unwrap().error.clone().unwrap();
        assert!(
            error.contains("Unavailable with --socket or --dev"),
            "{error}"
        );
        assert!(view.menu.page.is_none());
    });
}

/// Add existing works on a Window that is not the shown one: the menu switches
/// to it and then hands the picker over, so the chosen session lands in the
/// Window the user pointed at and not the previously active one.
#[gpui::test]
fn add_existing_targets_the_window_the_menu_was_opened_on(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_windows_development(cx);
    assert_eq!(active_id(&view, cx), full);
    right_click_tab(cx, empty);
    assert!(menu_open(&view, cx));
    click(cx, "container-menu-add-session");
    assert!(!menu_open(&view, cx));
    assert!(picker_open(&view, cx));
    assert_eq!(active_id(&view, cx), empty);
}

/// Choosing a row after that handoff attaches to the pointed-at Window only;
/// the previously active Window keeps its existing members untouched.
#[gpui::test]
fn adding_a_session_to_another_window_leaves_the_previous_one_untouched(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_windows_development(cx);
    cx.simulate_resize(size(px(640.), px(360.)));
    view.update(cx, |view, cx| {
        view.rename_container(full, &"W".repeat(64), cx).unwrap();
    });
    draw(cx);
    assert_eq!(active_id(&view, cx), full);
    let before_full = view.read_with(cx, |view, _| {
        view.containers.set().get(full).unwrap().members.clone()
    });
    right_click_tab(cx, empty);
    click(cx, "container-menu-add-session");
    draw(cx);
    // At 360px high, the project explanatory header precedes the first row.
    // Scroll the actual list as a user would, rather than clicking clipped
    // debug bounds (which exist even for off-screen elements).
    let list = bounds_of(cx, "sessions-list").unwrap();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-80.))),
        ..Default::default()
    });
    draw(cx);
    let row = bounds_of(cx, "sessions-row-0").unwrap();
    let project = bounds_of(cx, "sessions-project-0").unwrap();
    // The row has independent Project/Stop/Delete actions; click its label
    // side, not its center, which may legitimately belong to Project.
    let label = gpui::point(row.left() + px(12.), row.center().y);
    let list = bounds_of(cx, "sessions-list").unwrap();
    assert!(
        list.contains(&label),
        "the session label is visible: {row:?} in {list:?}"
    );
    assert!(!project.contains(&label));
    cx.simulate_click(label, Modifiers::default());
    draw(cx);
    view.read_with(cx, |view, _| {
        let empty_members = &view.containers.set().get(empty).unwrap().members;
        assert_eq!(
            empty_members,
            &[crate::containers::session_ref(
                crate::endpoint::LOCAL,
                "keep"
            )]
        );
        assert_eq!(
            view.containers.active().selected.as_ref(),
            empty_members.first()
        );
        let full_members = &view.containers.set().get(full).unwrap().members;
        assert_eq!(
            *full_members, before_full,
            "the other Window kept its members"
        );
    });
}

/// Opening the picker from the tab menu leaves a single overlay in control of
/// the screen; closing it returns focus to the window.
#[gpui::test]
fn only_one_overlay_survives_the_handoff(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows_development(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-add-session");
    assert!(!menu_open(&view, cx));
    assert!(picker_open(&view, cx));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!picker_open(&view, cx));
    cx.update(|window, cx| assert!(view.read(cx).focus.is_focused(window)));
}

/// The native Sessions command run from the tab menu of a Window that is not
/// the shown one closes that menu, hands the screen to the picker, and leaves
/// the Window on screen where it was: the command belongs to the shown Window,
/// not to the tab the menu was opened on.
#[gpui::test]
fn the_sessions_command_closes_the_tab_menu_and_keeps_the_shown_window(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_windows_development(cx);
    assert_eq!(active_id(&view, cx), full);
    right_click_tab(cx, empty);
    assert!(menu_open(&view, cx));
    assert_eq!(
        active_id(&view, cx),
        full,
        "opening the menu switches nothing"
    );
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(crate::RunCommand {
                command: crate::controls::Command::Sessions,
            }),
            cx,
        );
    });
    cx.run_until_parked();
    draw(cx);
    assert!(!menu_open(&view, cx), "the tab menu handed the screen over");
    assert!(picker_open(&view, cx));
    assert_eq!(
        active_id(&view, cx),
        full,
        "the picker belongs to the Window on screen, not the tab menu's"
    );
    // Typing reaches the picker's search field, so it holds the keyboard: a
    // query with no match paints the empty result, and clearing it brings the
    // only session back.
    cx.simulate_input("zzz");
    draw(cx);
    assert!(bounds_of(cx, "sessions-no-matches").is_some());
    assert!(bounds_of(cx, "sessions-row-0").is_none());
    cx.simulate_keystrokes("backspace backspace backspace");
    draw(cx);
    assert!(bounds_of(cx, "sessions-no-matches").is_none());
    assert!(bounds_of(cx, "sessions-row-0").is_some());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!picker_open(&view, cx));
    assert!(!menu_open(&view, cx));
    cx.update(|window, cx| assert!(view.read(cx).focus.is_focused(window)));
}

/// Choosing a session from the picker the native command opened attaches it to
/// the Window on screen; the tab the menu was opened on stays empty.
#[gpui::test]
fn a_session_chosen_after_the_native_command_lands_in_the_shown_window(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_windows_development(cx);
    assert_eq!(active_id(&view, cx), full);
    let before_full = view.read_with(cx, |view, _| {
        view.containers.set().get(full).unwrap().members.clone()
    });
    view.read_with(cx, |view, _| {
        assert!(view.containers.set().get(empty).unwrap().members.is_empty());
    });
    right_click_tab(cx, empty);
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(crate::RunCommand {
                command: crate::controls::Command::Sessions,
            }),
            cx,
        );
    });
    cx.run_until_parked();
    // The project header precedes the first row; scroll the actual list as a
    // user would rather than clicking clipped debug bounds, which exist even
    // for off-screen elements.
    let list = bounds_of(cx, "sessions-list").unwrap();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-80.))),
        ..Default::default()
    });
    draw(cx);
    // The row has independent Project/Stop/Delete actions; click its label
    // side, not its center, which may legitimately belong to Project.
    let row = bounds_of(cx, "sessions-row-0").unwrap();
    let label = gpui::point(row.left() + px(12.), row.center().y);
    let list = bounds_of(cx, "sessions-list").unwrap();
    assert!(
        list.contains(&label),
        "the session label is visible: {row:?} in {list:?}"
    );
    assert!(
        !bounds_of(cx, "sessions-project-0")
            .unwrap()
            .contains(&label)
    );
    cx.simulate_click(label, Modifiers::default());
    draw(cx);
    view.read_with(cx, |view, _| {
        let keep = crate::containers::session_ref(crate::endpoint::LOCAL, "keep");
        let full_members = &view.containers.set().get(full).unwrap().members;
        assert!(full_members.contains(&keep));
        assert_eq!(full_members.len(), before_full.len() + 1);
        assert_eq!(&full_members[..before_full.len()], &before_full[..]);
        assert!(
            view.containers.set().get(empty).unwrap().members.is_empty(),
            "the Window the menu was opened on stays empty"
        );
        assert_eq!(view.containers.active().id, full);
    });
}
