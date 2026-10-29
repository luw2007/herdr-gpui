#![allow(clippy::unwrap_used)]

//! Closing a logical Window asks a second time. The first request (the menu's
//! Close row, the tab's ×, or `herdr-gpui container close`) leaves the Window
//! there; Cancel, Escape and the backdrop keep it, and only Close Window
//! closes the one the question named.

use super::{bounds_of, click, draw, menu_open, right_click_tab, two_windows};
use crate::{HerdrWindow, containers::ContainerId};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

/// Whether the menu is showing the Close Window? step rather than the rows.
fn confirming(cx: &mut VisualTestContext) -> bool {
    bounds_of(cx, "container-menu-close-confirm").is_some()
        && bounds_of(cx, "container-menu-close-cancel").is_some()
}

fn present(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, id: ContainerId) -> bool {
    view.read_with(cx, |view, _| view.containers.set().get(id).is_some())
}

fn window_count(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.containers.containers().len())
}

fn focused(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.read(cx).focus.is_focused(window))
}

#[gpui::test]
fn the_tab_close_button_asks_before_it_closes_an_empty_window(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    click(cx, &format!("container-close-{empty}"));
    assert!(menu_open(&view, cx));
    assert!(confirming(cx));
    assert!(present(&view, cx, empty), "asking closes nothing");
    assert_eq!(window_count(&view, cx), 2);
}

#[gpui::test]
fn cancelling_the_close_keeps_the_window_and_returns_the_keyboard(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_windows(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-close");
    assert!(confirming(cx));
    assert!(present(&view, cx, empty));
    click(cx, "container-menu-close-cancel");
    assert!(present(&view, cx, empty));
    assert!(present(&view, cx, full));
    assert!(!menu_open(&view, cx));
    assert!(focused(&view, cx));
}

#[gpui::test]
fn escape_and_the_backdrop_cancel_the_close(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-close");
    assert!(confirming(cx));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(present(&view, cx, empty));
    assert!(!menu_open(&view, cx));
    assert!(focused(&view, cx));

    // A click outside the panel is the same answer.
    click(cx, &format!("container-close-{empty}"));
    assert!(confirming(cx));
    // The panel hangs under the strip, so the bottom-right corner is backdrop.
    cx.simulate_click(point(px(636.), px(356.)), Modifiers::default());
    draw(cx);
    assert!(present(&view, cx, empty));
    assert!(!menu_open(&view, cx));
    assert!(focused(&view, cx));
}

#[gpui::test]
fn confirming_closes_only_the_window_it_asked_about(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_windows(cx);
    let before = view.read_with(cx, |view, _| {
        (
            view.containers.set().get(full).unwrap().members.clone(),
            view.containers.set().get(empty).unwrap().members.clone(),
        )
    });
    right_click_tab(cx, empty);
    click(cx, "container-menu-close");
    assert!(confirming(cx));
    click(cx, "container-menu-close-confirm");
    assert!(!present(&view, cx, empty));
    assert!(present(&view, cx, full));
    assert_eq!(window_count(&view, cx), 1);
    view.read_with(cx, |view, _| {
        assert_eq!(view.containers.set().get(full).unwrap().members, before.0);
    });
    assert!(!menu_open(&view, cx));
    assert!(focused(&view, cx));
}

#[gpui::test]
fn enter_on_the_close_row_only_asks_and_a_second_enter_cancels(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    right_click_tab(cx, empty);
    view.update(cx, |view, cx| {
        let index = super::super::ROWS
            .iter()
            .position(|row| *row == super::super::Row::Close)
            .unwrap();
        view.container_menu.as_mut().unwrap().highlight = index;
        cx.notify();
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(confirming(cx));
    assert!(present(&view, cx, empty));
    // Enter never closes from the confirmation; the button is the only yes.
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(present(&view, cx, empty));
    assert!(!menu_open(&view, cx));
    assert!(focused(&view, cx));
}

#[gpui::test]
fn a_session_attached_after_the_question_keeps_the_window_and_says_why(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-close");
    assert!(confirming(cx));
    // Another native window (or the CLI) adds a session meanwhile.
    let session = crate::containers::session_ref("local", "added");
    view.update(cx, |view, cx| {
        view.edit_containers(cx, |set| set.attach(empty, session).map(drop))
            .unwrap();
    });
    click(cx, "container-menu-close-confirm");
    assert!(
        present(&view, cx, empty),
        "a non-empty Window is not closed"
    );
    assert!(menu_open(&view, cx));
    view.read_with(cx, |view, _| {
        let error = view.container_menu.as_ref().unwrap().error.clone().unwrap();
        assert!(error.contains("Detach its sessions first"), "{error}");
    });
}

#[gpui::test]
fn a_window_closed_elsewhere_leaves_the_question_nothing_to_close(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_windows(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-close");
    assert!(confirming(cx));
    view.update(cx, |view, cx| {
        view.close_container(empty, cx).unwrap();
    });
    draw(cx);
    draw(cx);
    assert!(!menu_open(&view, cx));
    assert!(!present(&view, cx, empty));
}
