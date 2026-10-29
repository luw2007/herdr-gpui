use super::*;
use crate::{
    sidebar::layout_tests::REPO_KEY,
    worktree_notes::{Checkout, Notes},
};

fn placeholder(view: &Entity<HerdrWindow>, cx: &mut gpui::VisualTestContext) -> String {
    view.read_with(cx, |view, cx| {
        view.sidebar_search.input.read(cx).placeholder().to_owned()
    })
}

#[gpui::test]
fn the_heading_is_the_field_and_spans_a_rows_highlight(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(connected);
    cx.simulate_resize(size(px(800.), px(700.)));
    cx.run_until_parked();
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    let field = cx.debug_bounds("sidebar-search").unwrap();
    let highlight = cx.debug_bounds("highlight-herdr").unwrap();
    assert_eq!(field.left(), highlight.left());
    assert_eq!(field.right(), highlight.right());
    let heading = cx.debug_bounds("header-workspaces").unwrap();
    let list = cx.debug_bounds("spaces-scroll").unwrap();
    assert_eq!(list.top(), heading.bottom(), "no line besides the heading");
    let label = placeholder(&view, cx);
    assert!(cx.debug_bounds("sidebar-search-clear").is_none());

    cx.update(|window, cx| {
        let focus = view.read(cx).sidebar_search.input.read(cx).focus.clone();
        window.focus(&focus, cx);
        full_draw(window, cx).clear(cx);
    });
    assert_eq!(
        placeholder(&view, cx),
        "Devices, worktrees, branches, notes"
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(view.read(cx).focus.is_focused(window));
        full_draw(window, cx).clear(cx);
    });
    assert_eq!(placeholder(&view, cx), label);
}

#[gpui::test]
fn a_query_finds_a_note_and_enter_opens_its_workspace(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(connected);
    cx.simulate_resize(size(px(800.), px(700.)));
    cx.update(|_, cx| {
        let endpoint = view.read(cx).endpoints[0].id.clone();
        Notes::update(cx, |notes| {
            notes.set(
                Checkout {
                    endpoint,
                    repo_key: REPO_KEY.into(),
                    branch: "worktree/sidebar-child".into(),
                },
                "waiting on the design review",
            )
        });
    });
    type_query(&view, "design review", cx);
    assert!(cx.debug_bounds("sidebar-search-hit-0").is_some());
    assert!(cx.debug_bounds("sidebar-search-hit-1").is_none());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.pending_navigation,
            Some(crate::NavigationTarget::Workspace("w4".into()))
        );
    });
}

/// The sidebar is a cached view, so focus moving into or out of the empty
/// field has to invalidate it: an ordinary draw, without the forced refresh
/// `full_draw` does, must still swap the heading for the hint and back.
#[gpui::test]
fn focus_and_blur_redraw_the_cached_heading(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(connected);
    cx.simulate_resize(size(px(800.), px(700.)));
    cx.run_until_parked();
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    let label = placeholder(&view, cx);
    let renders = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| view.read(cx).sidebar_view.read(cx).renders)
    };
    let before = renders(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(renders(cx), before, "an idle draw reuses the cache");

    let field = cx.debug_bounds("sidebar-search").unwrap();
    cx.simulate_click(field.center(), Modifiers::none());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(renders(cx) > before, "focusing re-renders the sidebar");
    assert_eq!(
        placeholder(&view, cx),
        "Devices, worktrees, branches, notes"
    );

    let before = renders(cx);
    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(renders(cx) > before, "blurring re-renders the sidebar");
    assert_eq!(placeholder(&view, cx), label);
}
