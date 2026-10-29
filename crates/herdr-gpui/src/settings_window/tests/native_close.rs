use super::*;
use gpui::VisualTestContext;

/// Settings asks before its native close button, menu entry, or shortcut can
/// drop the window: cancel keeps it and writes nothing, while agreeing saves
/// the pending layout draft before the window goes.
#[gpui::test]
fn native_close_asks_then_saves_before_closing(cx: &mut TestAppContext) {
    use crate::config::LayoutMode;
    let main = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    let writes = Arc::new(Mutex::new(Vec::new()));
    let settings = cx.update(|cx| {
        let weak = main.update(cx, |_, _, cx| cx.weak_entity()).unwrap();
        open_fixture(weak, cx);
        let settings = cx.global::<SettingsWindowHandle>().window.unwrap();
        settings
            .update(cx, |view, _, cx| {
                view.layout_io = Some(recording_layouts(writes.clone(), false));
                view.accept_layout_choice(LayoutMode::Minimal, cx);
            })
            .unwrap();
        settings
    });
    let settings: AnyWindowHandle = settings.into();
    let mut visual = VisualTestContext::from_window(settings, cx);

    assert!(
        !visual.simulate_close(),
        "the native close waits for an answer"
    );
    visual.run_until_parked();
    assert!(visual.has_pending_prompt());
    visual.simulate_prompt_answer("Cancel");
    visual.run_until_parked();
    assert!(
        visual.windows().contains(&settings),
        "cancel keeps Settings open"
    );
    assert!(writes.lock().unwrap().is_empty(), "cancel writes no draft");

    assert!(!visual.simulate_close());
    visual.run_until_parked();
    assert!(
        visual.has_pending_prompt(),
        "a cancel leaves no prompt behind"
    );
    visual.simulate_prompt_answer("Close");
    visual.run_until_parked();
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(10));
    visual.run_until_parked();
    assert_eq!(*writes.lock().unwrap(), [LayoutMode::Minimal]);
    assert_eq!(
        visual.windows(),
        [main.into()],
        "agreeing closes Settings after its save and only Settings"
    );
}
