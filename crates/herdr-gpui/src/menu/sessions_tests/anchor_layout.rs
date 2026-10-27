//! The real strip and footer entry points share a viewport-bounded picker.
use super::*;
use gpui::{ScrollDelta, ScrollWheelEvent, point};

fn fixture(cx: &mut TestAppContext) -> (Entity<crate::HerdrWindow>, &mut VisualTestContext) {
    cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.endpoints[0].connection.target = development("s000");
        view.containers = crate::containers::ContainerView::local(&view.endpoints[0].session_ref());
        view.sessions.entries = (0..40)
            .map(|index| session(&format!("s{index:03}"), SessionState::Stopped))
            .collect();
        view
    })
}

fn assert_layout(cx: &mut VisualTestContext, viewport: gpui::Size<Pixels>) {
    let panel = bounds(cx, "menu-panel");
    let toolbar = bounds(cx, "sessions-toolbar");
    let list = bounds(cx, "sessions-list");
    assert!(panel.left() >= px(0.) && panel.right() <= viewport.width);
    assert!(panel.top() >= px(0.) && panel.bottom() <= viewport.height);
    assert!(list.left() >= panel.left() && list.right() <= panel.right());
    assert!(list.top() >= toolbar.bottom() && list.bottom() <= panel.bottom());
    assert!(
        list.size.height >= px(80.),
        "rows get usable room: {list:?}"
    );
    let first = bounds(cx, "sessions-row-0");
    cx.simulate_event(ScrollWheelEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-120.))),
        ..Default::default()
    });
    draw(cx);
    assert!(bounds(cx, "sessions-row-0").top() < first.top());
    assert_eq!(bounds(cx, "menu-panel"), panel);
    assert_eq!(bounds(cx, "sessions-toolbar"), toolbar);
}

#[gpui::test]
fn the_top_strip_picker_uses_room_below_at_regular_and_narrow_sizes(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    for viewport in [size(px(800.), px(600.)), size(px(360.), px(360.))] {
        view.update(cx, |view, _| view.menu.reset());
        cx.simulate_resize(viewport);
        cx.run_until_parked();
        draw(cx);
        let add = bounds(cx, "container-strip-add-session");
        click(cx, "container-strip-add-session");
        draw(cx);
        assert!(bounds(cx, "menu-panel").top() >= add.center().y);
        if viewport.height == px(600.) {
            assert!(bounds(cx, "sessions-list").size.height >= px(280.));
        }
        assert_layout(cx, viewport);
    }
}

#[gpui::test]
fn the_bottom_footer_picker_opens_upward_at_regular_and_narrow_sizes(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    for viewport in [size(px(800.), px(600.)), size(px(360.), px(360.))] {
        view.update(cx, |view, _| view.menu.reset());
        cx.simulate_resize(viewport);
        cx.run_until_parked();
        draw(cx);
        let footer = bounds(cx, "device-footer");
        click(cx, "device-sessions");
        draw(cx);
        assert!(bounds(cx, "menu-panel").bottom() <= footer.top());
        assert_layout(cx, viewport);
    }
}
