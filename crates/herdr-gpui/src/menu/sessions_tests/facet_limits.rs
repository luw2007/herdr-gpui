//! A facet with hundreds of values must neither push the sessions out of the
//! panel nor leave a value out of reach.
use super::*;
use crate::projects::Projects;

use gpui::{ScrollDelta, ScrollWheelEvent, point};

const COUNT: usize = 300;

fn short_window(cx: &mut TestAppContext) -> (Entity<crate::HerdrWindow>, &mut VisualTestContext) {
    let (view, cx) = open_list(
        cx,
        vec![
            session("alpha", SessionState::Running),
            session("beta", SessionState::Running),
        ],
        development("default"),
    );
    // The popup anchors to the footer, so reopen it in the small window.
    cx.simulate_resize(size(px(640.), px(360.)));
    cx.run_until_parked();
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    click(cx, "device-sessions");
    draw(cx);
    (view, cx)
}

/// Opens `facet`, checks the list keeps room, scrolls the choices to their
/// end and returns the selector of the last one after confirming it is
/// inside the scroller.
fn assert_reachable(cx: &mut VisualTestContext, facet: &'static str, last: &'static str) {
    click(cx, facet);
    draw(cx);
    let list = bounds(cx, "sessions-list");
    assert!(
        list.size.height >= px(40.),
        "the sessions keep room: {:?}",
        list.size
    );
    let choices = bounds(cx, "sessions-facet-choices");
    assert!(choices.bottom() <= bounds(cx, "menu-panel").bottom());
    for _ in 0..80 {
        cx.simulate_event(ScrollWheelEvent {
            position: choices.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-400.))),
            ..Default::default()
        });
        draw(cx);
    }
    let tail = bounds(cx, last);
    assert!(
        tail.top() >= choices.top() && tail.bottom() <= choices.bottom(),
        "the last choice scrolls into view: {tail:?} in {choices:?}"
    );
    click(cx, last);
    draw(cx);
}

#[gpui::test]
fn hundreds_of_projects_scroll_and_the_last_is_pickable(cx: &mut TestAppContext) {
    let (_view, cx) = short_window(cx);
    for index in 0..COUNT {
        cx.update(|_, cx| {
            Projects::update(cx, |store| {
                store.create_project(&format!("Project {index:03}"), None)
            })
            .unwrap();
        });
    }
    assert_reachable(cx, "sessions-facet-Project", "sessions-choice-Project-301");
    // Neither session belongs to the last project, so choosing it empties the list.
    assert!(cx.debug_bounds("sessions-no-matches").is_some());
    assert!(cx.debug_bounds("sessions-facet-choices").is_none());
}

#[gpui::test]
fn many_devices_scroll_and_the_last_is_pickable(cx: &mut TestAppContext) {
    let (view, cx) = short_window(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            for index in 0..COUNT {
                view.endpoints.push(remote(
                    &format!("ssh:d{index}"),
                    &format!("Device {index:03}"),
                    "host.invalid",
                    "default",
                ));
            }
            view.list_all_endpoints();
        })
    });
    draw(cx);
    let count = cx.update(|_, cx| {
        view.read(cx)
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.slot.is_none())
            .count()
    });
    let last: &'static str = Box::leak(format!("sessions-choice-Device-{count}").into_boxed_str());
    assert_reachable(cx, "sessions-facet-Device", last);
    let picked = cx.update(|_, cx| view.read(cx).session_scope().facets.device);
    assert_eq!(picked.as_deref(), Some(&*format!("ssh:d{}", COUNT - 1)));
    assert!(cx.debug_bounds("sessions-facet-choices").is_none());
}
