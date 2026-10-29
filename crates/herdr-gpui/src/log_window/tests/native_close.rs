use super::*;

/// The Log window asks before closing, from its close action or the native
/// close button, and only ever closes itself.
#[gpui::test]
fn close_action_asks_and_cancel_keeps_logs(cx: &mut TestAppContext) {
    let _source = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    let (_, cx) = cx.add_window_view(|window, cx| {
        crate::native_close::install(window, cx);
        LogWindow::new(window, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.dispatch_action(Close);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "closing Logs asks first");

    // A repeat request while the prompt is open stacks no second prompt.
    cx.dispatch_action(Close);
    cx.run_until_parked();
    assert!(cx.pending_prompt().is_some());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt(), "one prompt takes one answer");
    assert_eq!(
        cx.windows().len(),
        2,
        "cancel keeps Logs and the window it came from"
    );
}

#[gpui::test]
fn agreed_native_close_removes_only_logs(cx: &mut TestAppContext) {
    let source = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    let (_, cx) = cx.add_window_view(|window, cx| {
        crate::native_close::install(window, cx);
        LogWindow::new(window, cx)
    });
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "the native close asks first");
    cx.simulate_prompt_answer("Close");
    cx.run_until_parked();
    assert_eq!(cx.windows(), [source.into()]);
}
