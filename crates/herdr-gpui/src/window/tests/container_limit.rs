//! A refused New Space reports its registry limit through the command and strip.
//! Fixed strip controls remain reachable with many Spaces and long titles.
use super::*;
use crate::containers::MAX_CONTAINERS;
use gpui::{Bounds, Modifiers, Pixels, VisualTestContext, px, size};

fn fill(view: &mut HerdrWindow, cx: &mut gpui::Context<HerdrWindow>) {
    while view.containers.containers().len() < MAX_CONTAINERS {
        view.new_container(None, cx).unwrap();
    }
    view.local_error = None;
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
}

fn failure_toast(view: &gpui::Entity<HerdrWindow>, cx: &mut VisualTestContext) -> Option<u64> {
    view.read_with(cx, |view, _| {
        view.endpoints[view.selected_endpoint]
            .toasts
            .entries
            .iter()
            .find(|(_, notice)| notice.is_local_feedback())
            .map(|(id, _)| *id)
    })
}

/// Whether the failure toast is on screen, found by its drawn element.
fn toast_drawn(view: &gpui::Entity<HerdrWindow>, cx: &mut VisualTestContext) -> bool {
    let Some(id) = failure_toast(view, cx) else {
        return false;
    };
    let endpoint = view.read_with(cx, |view, _| {
        view.endpoints[view.selected_endpoint].id.clone()
    });
    draw(cx);
    cx.debug_bounds(Box::leak(format!("toast-{endpoint}-{id}").into_boxed_str()))
        .is_some()
}

fn within(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.left() >= outer.left()
        && inner.right() <= outer.right()
        && inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
}

#[gpui::test]
fn the_new_window_command_reports_a_full_registry(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update_in(cx, |view, window, cx| {
        fill(view, cx);
        view.command(Command::NewContainer, window, cx);
        let shown = view.endpoints[view.selected_endpoint]
            .toasts
            .entries
            .back()
            .unwrap()
            .1
            .body
            .clone()
            .unwrap();
        assert!(shown.contains(&MAX_CONTAINERS.to_string()), "{shown}");
        assert_eq!(view.containers.containers().len(), MAX_CONTAINERS);
    });
}

#[gpui::test]
fn the_strip_button_reports_a_full_registry(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(1000.), px(600.)));
    view.update(cx, fill);
    cx.run_until_parked();
    draw(cx);
    let point = cx.debug_bounds("container-new").unwrap().center();
    cx.simulate_click(point, Modifiers::default());
    assert!(toast_drawn(&view, cx));
    let body = view.read_with(cx, |view, _| {
        view.endpoints[view.selected_endpoint]
            .toasts
            .entries
            .back()
            .unwrap()
            .1
            .body
            .clone()
            .unwrap()
    });
    assert!(body.contains(&MAX_CONTAINERS.to_string()), "{body}");
}

#[gpui::test]
fn a_refusal_shows_while_disconnected_without_replacing_the_reason(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(1000.), px(600.)));
    view.update(cx, |view, cx| {
        fill(view, cx);
        view.live.status = crate::state::ConnectionStatus::Disconnected;
        view.live.error = Some("socket closed".into());
    });
    draw(cx);
    let point = cx.debug_bounds("container-new").unwrap().center();
    cx.simulate_click(point, Modifiers::default());
    assert!(toast_drawn(&view, cx));
    view.read_with(cx, |view, _| {
        assert_eq!(view.live.error.as_deref(), Some("socket closed"));
        assert!(view.local_error.is_none());
    });
}

#[gpui::test]
fn a_refusal_shows_with_the_status_bar_hidden(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(1000.), px(600.)));
    view.update_in(cx, |view, window, cx| {
        fill(view, cx);
        view.command(Command::ToggleStatusBar, window, cx);
        assert!(!view.status_bar_visible);
        view.command(Command::NewContainer, window, cx);
    });
    draw(cx);
    assert!(cx.debug_bounds("connection-status").is_none());
    assert!(toast_drawn(&view, cx));
}

#[gpui::test]
fn a_later_success_clears_the_old_refusal(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(1000.), px(600.)));
    view.update_in(cx, |view, window, cx| {
        fill(view, cx);
        view.command(Command::NewContainer, window, cx);
        view.command(Command::NewContainer, window, cx);
    });
    assert!(toast_drawn(&view, cx));
    let count = view.read_with(cx, |view, _| {
        view.endpoints[view.selected_endpoint]
            .toasts
            .entries
            .iter()
            .filter(|(_, notice)| notice.is_local_feedback())
            .count()
    });
    assert_eq!(count, 1, "repeated refusals replace one another");
    view.update_in(cx, |view, window, cx| {
        let last = view.containers.containers().last().unwrap().id;
        view.close_container(last, cx).unwrap();
        view.command(Command::NewContainer, window, cx);
    });
    assert!(failure_toast(&view, cx).is_none());
    assert!(!toast_drawn(&view, cx));
}

/// Windows each shown at an ordinary 1000px width: the buttons stay inside
/// the strip, which stays inside the viewport.
fn assert_reachable(view: &gpui::Entity<HerdrWindow>, cx: &mut VisualTestContext) {
    draw(cx);
    draw(cx);
    let viewport = Bounds::new(gpui::point(px(0.), px(0.)), size(px(1000.), px(600.)));
    let strip = cx.debug_bounds("container-strip").unwrap();
    assert!(within(strip, viewport), "{strip:?}");
    let new = cx.debug_bounds("container-new").unwrap();
    assert!(within(new, strip), "new {new:?} strip {strip:?}");
    let active = view.read_with(cx, |view, _| {
        view.containers.containers()[view.containers.active_index()].id
    });
    let menu = cx
        .debug_bounds(Box::leak(
            format!("container-menu-{active}").into_boxed_str(),
        ))
        .unwrap();
    let tabs = cx.debug_bounds("container-tabs").unwrap();
    assert!(within(menu, tabs), "menu {menu:?} tabs {tabs:?}");
    cx.simulate_click(menu.center(), Modifiers::default());
    assert!(view.read_with(cx, |view, _| view.container_menu.is_some()));
}

#[gpui::test]
fn many_windows_keep_new_and_the_menu_reachable(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(1000.), px(600.)));
    view.update(cx, fill);
    assert_eq!(
        view.read_with(cx, |view, _| view.containers.containers().len()),
        MAX_CONTAINERS
    );
    assert_reachable(&view, cx);
}

#[gpui::test]
fn a_long_cjk_title_is_cut_and_keeps_the_buttons_reachable(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(1000.), px(600.)));
    let title = "窗".repeat(64);
    view.update(cx, |view, cx| {
        let id = view.new_container(Some(&title), cx).unwrap();
        view.rename_container(id, &title, cx).unwrap();
    });
    draw(cx);
    let id = view.read_with(cx, |view, _| {
        view.containers.containers().last().unwrap().id
    });
    let tab = cx
        .debug_bounds(Box::leak(format!("container-tab-{id}").into_boxed_str()))
        .unwrap();
    assert!(tab.size.width <= px(221.), "{tab:?}");
    assert_eq!(
        view.read_with(cx, |view, _| view
            .containers
            .set()
            .get(id)
            .unwrap()
            .title
            .clone()),
        title,
        "the stored title is whole; only its drawing is cut"
    );
    assert_reachable(&view, cx);
}
