use super::*;

/// An empty logical Window shows no terminal, so no input path may reach the
/// session still connected behind it. Everything below is attempted while the
/// Window is empty; the first message the daemon sees must be the sentinel
/// sent after the original Window is shown again.
#[gpui::test]
fn empty_window_sends_nothing_to_the_hidden_session(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        Fixture(cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)))
    });
    let view = fixture.update(cx, |fixture, _| fixture.0.clone());
    let (endpoint, mut server) = connected_endpoint("hidden");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            prepare_mouse(view, endpoint, cx);
            // A flush records the target, so only the send guard stops held input.
            assert!(view.flush_pending_input(cx));
            let listed = view.containers.shown_id();
            view.new_container(Some("Empty"), cx).unwrap();
            assert!(view.container_is_empty());
            assert!(!view.input_ready());
            let item = ClipboardItem::new_string("rm -rf /\n".into());
            cx.write_to_clipboard(item.clone());
            // Edit > Paste, the native clipboard reservation, the text paste
            // and plain sends are all refused.
            view.paste(cx);
            view.paste_native_clipboard(false, None, cx);
            assert!(!view.paste_terminal_clipboard(item, false, cx));
            view.send(ClientPaneInputEvent::Paste("typed".into()), cx);
            view.send(ClientPaneInputEvent::TextCommit("typed".into()), cx);
            assert!(view.pending_images.is_empty());
            assert_eq!(view.pending_input.len(), 0);
            view.switch_container(listed, cx);
            assert!(!view.container_is_empty());
            view.send(ClientPaneInputEvent::TextCommit("sentinel".into()), cx);
        });
        let _ = window;
    });
    cx.run_until_parked();
    assert_eq!(
        server.receive(),
        ClientMessage::ClientShellPaneInput {
            pane_id: "w1:p1".into(),
            events: vec![ClientPaneInputEvent::TextCommit("sentinel".into())],
        }
    );
}
