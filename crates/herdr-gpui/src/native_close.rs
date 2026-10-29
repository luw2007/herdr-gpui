//! Second confirmation before a user closes a native window.
//!
//! Closing a window only detaches this client: the Herdr daemon and its
//! terminals keep running. Every user close path asks here first — the
//! platform's close button, its window menu, a shortcut, or a client-drawn
//! title bar — so a stray click cannot drop a window. A repeat request while
//! a prompt is open shows no second prompt and closes nothing.

use gpui::{AnyWindowHandle, App, Global, PromptButton, PromptLevel, Window};
use std::collections::HashSet;

/// The prompt's answers; Cancel comes first, where platforms put the default
/// button, so Return and Escape cancel rather than close.
const CANCEL: &str = "Cancel";
const CLOSE: &str = "Close";
/// Close is the second answer in [`request`]'s prompt.
const CLOSE_ANSWER: usize = 1;
const MESSAGE: &str = "Close this window?";
const DETAIL: &str = "Only this window closes; the daemon and its terminals keep running.";

/// Windows with a close prompt on screen.
#[derive(Default)]
struct Prompted(HashSet<AnyWindowHandle>);

impl Global for Prompted {}

/// Makes the platform's own close path ask first: its close button, window
/// menu, and shortcuts. A client-drawn title bar calls [`request_close`].
pub(crate) fn install(window: &mut Window, cx: &mut App) {
    window.on_window_should_close(cx, |window, cx| {
        request_close(window, cx);
        false
    });
}

/// Asks before closing `window`, which is removed once the user agrees.
/// Whether this request opened a prompt; a repeat while it is open does not.
pub(crate) fn request_close(window: &Window, cx: &mut App) -> bool {
    let handle = window.window_handle();
    request(handle, cx, move |cx| {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    })
}

/// Asks before closing `handle`, then runs `confirmed` if the user agrees.
/// Whether this request opened a prompt; a repeat while it is open does not,
/// so one window never shows two.
pub(crate) fn request(
    handle: AnyWindowHandle,
    cx: &mut App,
    confirmed: impl FnOnce(&mut App) + 'static,
) -> bool {
    let open = cx.windows();
    let prompted = cx.default_global::<Prompted>();
    // A window closed while its prompt was up leaves only a stale mark.
    prompted.0.retain(|window| open.contains(window));
    if !prompted.0.insert(handle) {
        return false;
    }
    let answers = [PromptButton::cancel(CANCEL), PromptButton::ok(CLOSE)];
    // A platform close arrives inside its own close event; asking there would
    // run a modal prompt re-entrantly, so ask once the window has been kept
    // open and that event has returned.
    cx.defer(move |cx| {
        let answer = handle.update(cx, |_, window, cx| {
            window.prompt(PromptLevel::Warning, MESSAGE, Some(DETAIL), &answers, cx)
        });
        let Ok(answer) = answer else {
            // The window went away before its prompt could appear.
            forget(handle, cx);
            return;
        };
        cx.spawn(async move |cx| {
            if matches!(answer.await, Ok(chosen) if chosen == CLOSE_ANSWER) {
                cx.update(confirmed);
            }
            // Cleared after the close runs, so a platform close that asks
            // again while it closes cannot open a second prompt.
            cx.update(|cx| forget(handle, cx));
        })
        .detach();
    });
    true
}

fn forget(handle: AnyWindowHandle, cx: &mut App) {
    cx.default_global::<Prompted>().0.remove(&handle);
}

#[cfg(test)]
mod tests;
