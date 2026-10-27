use super::*;
use gpui::{px, size};

#[gpui::test]
fn logical_window_tabs_clear_native_buttons_when_the_header_is_merged(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    for position in ["top", "bottom"] {
        view.update(cx, |view, _| {
            view.settings.shared = Some(
                crate::herdr_settings::Settings::parse_text(&format!(
                    "[ui]\ntab_bar_position = '{position}'"
                ))
                .unwrap(),
            );
        });
        for width in [640., 1200.] {
            cx.simulate_resize(size(px(width), px(600.)));
            cx.update(|window, cx| {
                crate::sidebar::layout_tests::full_draw(window, cx).clear(cx);
            });
            let strip = cx.debug_bounds("container-strip").unwrap();
            let tabs = cx.debug_bounds("container-tabs").unwrap();
            if position == "top" {
                assert!(cx.debug_bounds("titlebar").is_none());
                assert!(tabs.left() >= strip.left() + px(crate::titlebar::LEADING));
                assert!(strip.size.height >= px(crate::titlebar::HEIGHT));
            } else {
                let header = cx.debug_bounds("titlebar").unwrap();
                assert!(strip.top() >= header.bottom());
                assert_eq!(tabs.left(), strip.left() + px(8.));
            }
            let body = cx.debug_bounds("window-body").unwrap();
            assert!(body.top() >= strip.bottom());
        }
    }
}
