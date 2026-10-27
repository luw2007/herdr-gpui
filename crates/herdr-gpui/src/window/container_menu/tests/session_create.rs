//! Context-menu Create hands keyboard input and the pointed-at Window to the picker.

use super::{bounds_of, click, draw, menu_open, right_click_tab};
use crate::{
    HerdrWindow,
    containers::{ContainerId, ContainerView, Grouping, session_ref},
    menu::Page,
    sidebar::layout_tests::fixture_window,
};
use gpui::{Entity, TestAppContext, VisualTestContext, px, size};
use herdr_client::ConnectTarget;

fn local(name: &str) -> ConnectTarget {
    ConnectTarget::Session {
        name: name.into(),
        development: false,
    }
}

fn two_local_windows(
    cx: &mut TestAppContext,
) -> (
    Entity<HerdrWindow>,
    &mut VisualTestContext,
    ContainerId,
    ContainerId,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.endpoints[0].connection.target = local("keep");
        view.containers = ContainerView::local(&view.endpoints[0].session_ref());
        view
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    let (full, empty) = view.update(cx, |view, cx| {
        let full = view.containers.active().id;
        let empty = view.new_container(Some("Second"), cx).unwrap();
        assert!(view.switch_container(full, cx));
        (full, empty)
    });
    draw(cx);
    (view, cx, full, empty)
}

#[gpui::test]
fn create_on_a_non_active_window_receives_typed_name_and_attaches_a_local_session(
    cx: &mut TestAppContext,
) {
    let (view, cx, full, empty) = two_local_windows(cx);
    let kept = view.read_with(cx, |view, _| {
        view.containers.set().get(full).unwrap().members.clone()
    });
    right_click_tab(cx, empty);
    click(cx, "container-menu-create-session");
    assert!(!menu_open(&view, cx));
    assert!(bounds_of(cx, "session-modal").is_some());
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu.page, Some(Page::Sessions));
        assert_eq!(view.containers.active().id, empty);
    });

    // Text input is delivered to the focused input handler, not set through
    // private picker state: successful submission proves the name field owns it.
    cx.simulate_input("context-created");
    cx.simulate_keystrokes("enter");
    draw(cx);
    view.read_with(cx, |view, _| {
        let created = session_ref(crate::endpoint::LOCAL, "context-created");
        let target = view.containers.set().get(empty).unwrap();
        assert!(target.members.contains(&created));
        assert_eq!(target.selected.as_ref(), Some(&created));
        assert_eq!(view.containers.set().get(full).unwrap().members, kept);
        let endpoint = &view.endpoints[view.selected_endpoint];
        assert_eq!(endpoint.session_ref(), created);
        assert_eq!(endpoint.slot.as_ref(), Some(&created));
        assert_eq!(endpoint.connection.target, local("context-created"));
        assert!(view.menu.page.is_none());
    });
}

#[gpui::test]
fn escaping_the_name_form_restores_picker_focus_then_window_focus(cx: &mut TestAppContext) {
    let (view, cx, _, empty) = two_local_windows(cx);
    right_click_tab(cx, empty);
    click(cx, "container-menu-create-session");
    let name_focus = cx.update(|window, cx| window.focused(cx).unwrap());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(bounds_of(cx, "session-modal").is_none());
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu.page, Some(Page::Sessions))
    });
    let picker_focus = cx.update(|window, cx| window.focused(cx).unwrap());
    assert_ne!(name_focus, picker_focus);
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.update(|window, cx| {
        let view = view.read(cx);
        assert!(view.menu.page.is_none());
        assert!(view.focus.is_focused(window));
    });
}

#[gpui::test]
fn create_is_refused_while_a_session_mutation_is_in_flight(cx: &mut TestAppContext) {
    let (view, cx, full, empty) = two_local_windows(cx);
    view.update(cx, |view, cx| {
        // Same pending foreground-task fixture used for an in-flight config load.
        view.sessions.mutation = Some(cx.spawn(async |_, _| std::future::pending::<()>().await));
    });
    right_click_tab(cx, empty);
    view.update(cx, |view, cx| {
        view.container_menu.as_mut().unwrap().highlight = super::super::ROWS
            .iter()
            .position(|row| *row == super::super::Row::CreateSession)
            .unwrap();
        cx.notify();
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(menu_open(&view, cx));
    assert!(bounds_of(cx, "container-menu-error").is_some());
    view.read_with(cx, |view, _| {
        assert!(view.menu.page.is_none());
        assert_eq!(view.containers.active().id, full);
        assert_eq!(view.container_menu.as_ref().unwrap().id, empty);
        assert_eq!(view.endpoints[0].connection.target, local("keep"));
        assert!(view.sessions.mutation.is_some());
    });
}

#[gpui::test]
fn mouse_click_on_disabled_create_does_not_open_picker_or_switch_its_target(
    cx: &mut TestAppContext,
) {
    let (view, cx, full, empty) = two_local_windows(cx);
    view.update(cx, |view, cx| {
        view.sessions.mutation = Some(cx.spawn(async |_, _| std::future::pending::<()>().await));
    });
    right_click_tab(cx, empty);
    click(cx, "container-menu-create-session");
    assert!(menu_open(&view, cx));
    view.read_with(cx, |view, _| {
        assert!(view.menu.page.is_none());
        assert_eq!(view.containers.active().id, full);
        assert_eq!(view.container_menu.as_ref().unwrap().id, empty);
        assert_eq!(view.endpoints[0].connection.target, local("keep"));
    });
}

#[gpui::test]
fn large_font_keyboard_navigation_reaches_and_activates_the_last_grouping_row(
    cx: &mut TestAppContext,
) {
    let (view, cx, _, empty) = two_local_windows(cx);
    cx.simulate_resize(size(px(640.), px(360.)));
    view.update(cx, |view, cx| {
        view.config.ui.size = 24.;
        cx.notify();
    });
    draw(cx);
    right_click_tab(cx, empty);
    let mr = super::super::ROWS
        .iter()
        .position(|row| *row == super::super::Row::Group(Grouping::Mr))
        .unwrap();
    for _ in 0..mr {
        cx.simulate_keystrokes("down");
        draw(cx);
    }
    let row = bounds_of(cx, "container-menu-group-mr").unwrap();
    assert!(row.top() >= px(0.) && row.bottom() <= px(360.));
    cx.simulate_keystrokes("enter");
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.containers.set().get(empty).unwrap().grouping,
            Grouping::Mr
        );
    });
    assert!(!menu_open(&view, cx));
}
