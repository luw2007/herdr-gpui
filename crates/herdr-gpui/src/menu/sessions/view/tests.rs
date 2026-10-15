//! IME behavior uses the real SearchInput handler and window event dispatch.
use crate::menu::{
    Page,
    sessions::Row,
    sessions_tests::{development, draw, open_list, session},
};
use gpui::{EntityInputHandler, KeyDownEvent, Keystroke, PlatformInput, TestAppContext};
use herdr_client::SessionState;

#[gpui::test]
fn composition_navigation_cannot_choose_or_dismiss_until_committed(cx: &mut TestAppContext) {
    let (view, cx) = open_list(
        cx,
        vec![
            session("alpha", SessionState::Stopped),
            session("beta", SessionState::Stopped),
        ],
        development("alpha"),
    );
    let search = cx.update(|_, cx| {
        view.read(cx)
            .menu
            .sessions_view
            .as_ref()
            .unwrap()
            .search
            .clone()
    });
    cx.update(|window, cx| {
        search.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "beta", Some(4..4), window, cx);
        });
    });
    cx.run_until_parked();
    draw(cx);
    // Marking text may update its preview matches; navigation keys must not
    // change that preview, selection, target, or the manager's visibility.
    let before = cx.update(|_, cx| {
        let view = view.read(cx);
        (
            view.session_query().to_owned(),
            view.menu.selected,
            view.session_entries()
                .iter()
                .filter_map(|entry| entry.row().cloned())
                .collect::<Vec<Row>>(),
        )
    });
    for key in ["enter", "up", "down", "escape"] {
        cx.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: Keystroke::parse(key).unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
        });
        cx.update(|_, cx| {
            let view = view.read(cx);
            assert_eq!(view.menu.page, Some(Page::Sessions), "{key}");
            assert_eq!(
                view.endpoints[0].connection.target,
                development("alpha"),
                "{key}"
            );
            assert_eq!(view.session_query(), before.0, "{key}");
            assert_eq!(view.menu.selected, before.1, "{key}");
            assert_eq!(
                view.session_entries()
                    .iter()
                    .filter_map(|entry| entry.row().cloned())
                    .collect::<Vec<_>>(),
                before.2,
                "{key}"
            );
            assert!(search.read(cx).is_composing(), "{key}");
            assert!(view.menu.session_edit.is_none(), "{key}");
        });
    }
    cx.update(|window, cx| {
        search.update(cx, |input, cx| {
            input.replace_text_in_range(None, "beta", window, cx)
        });
    });
    cx.run_until_parked();
    draw(cx);
    cx.update(|_, cx| assert_eq!(view.read(cx).menu.page, Some(Page::Sessions)));
    cx.simulate_keystrokes("enter");
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(
            view.menu.page.is_none(),
            "only the post-commit Enter chooses"
        );
        // Development targets are attach-only, never a real daemon spawn.
        let shown = &view.endpoints[view.selected_endpoint];
        assert_eq!(shown.session_ref().session, "beta");
        assert_eq!(shown.connection.target, development("beta"));
    });
}
