use super::*;

/// Close Window asks first, then closes only the window it runs in: a tiling
/// compositor draws no close button, so the menu and keybinding are the way
/// out.
#[gpui::test]
fn close_window_closes_only_its_own_window(cx: &mut gpui::TestAppContext) {
    let first = cx.add_window(fixture_window);
    let second = cx.add_window(fixture_window);
    cx.update(|cx| {
        second
            .update(cx, |view, window, cx| {
                view.command(Command::CloseWindow, window, cx);
            })
            .unwrap();
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "close asks before it closes");
    assert_eq!(
        cx.update(|cx| cx.windows().len()),
        2,
        "the prompt closes nothing"
    );
    cx.simulate_prompt_answer("Close");
    cx.run_until_parked();
    let remaining = cx.update(main_windows);
    assert_eq!(remaining, [first]);
}
