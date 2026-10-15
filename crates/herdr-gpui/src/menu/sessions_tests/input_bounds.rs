//! Context at page boundaries and usable controls in a short session manager.
use super::*;
use crate::menu::sessions::{Entry, Row};
use gpui::{ScrollDelta, ScrollWheelEvent, point};
use herdr_client::SessionWorkspace;

fn catalog(count: usize) -> Vec<LocalSession> {
    (0..count)
        .map(|index| LocalSession {
            workspaces: vec![SessionWorkspace {
                project: "/code/project".into(),
                path: format!("/code/project.wt/s{index:03}"),
                branch: Some(format!("feature/s{index:03}")),
                label: None,
                repo: None,
            }],
            ..session(&format!("s{index:03}"), SessionState::Stopped)
        })
        .collect()
}

#[gpui::test]
fn the_second_page_starts_with_its_device_and_project_context(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(250), development("s000"));
    click(cx, "sessions-page-next");
    draw(cx);
    cx.update(|_, cx| {
        let entries = view.read(cx).session_entries();
        assert!(matches!(&entries[0], Entry::Header { text, .. } if text == "LOCAL SESSIONS"));
        assert!(matches!(&entries[1], Entry::Muted { selector, text }
            if selector == "sessions-project-local-Unassigned · /code/project"
                && text == "Unassigned · /code/project"));
        assert!(matches!(entries[2].row(), Some(Row::Local(name)) if name == "s100"));
    });
    let device = bounds(cx, "sessions-local-header");
    let project = bounds(cx, "sessions-project-local-Unassigned · /code/project");
    assert!(device.bottom() <= project.top());
    assert!(project.bottom() <= bounds(cx, "sessions-row-0").top());
}

#[gpui::test]
fn a_short_multiline_list_scrolls_to_its_end_without_covering_the_search(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(30), development("s000"));
    cx.simulate_resize(size(px(640.), px(360.)));
    cx.run_until_parked();
    draw(cx);
    let toolbar = bounds(cx, "sessions-toolbar");
    let list = bounds(cx, "sessions-list");
    assert!(list.top() >= toolbar.bottom());
    // Thirty real, multiline sessions are followed by the local Add action.
    cx.simulate_event(ScrollWheelEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
        ..Default::default()
    });
    draw(cx);
    let last = bounds(cx, "sessions-row-30");
    assert!(last.bottom() <= bounds(cx, "menu-panel").bottom());
    assert!(last.top() >= bounds(cx, "sessions-toolbar").bottom());
    assert_eq!(bounds(cx, "sessions-toolbar"), toolbar);
    click(cx, "sessions-search");
    cx.simulate_keystrokes("s 0 2 9");
    draw(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        let entries = view.session_entries();
        let names: Vec<_> = entries.iter().filter_map(Entry::row).collect();
        assert_eq!(names, vec![&Row::Local("s029".into())]);
        assert_eq!(view.menu.page, Some(Page::Sessions));
    });
}
