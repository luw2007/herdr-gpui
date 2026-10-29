#![allow(clippy::unwrap_used)]

use super::*;
use crate::{HerdrWindow, controls::Command, sidebar::layout_tests::fixture_window};
use gpui::{Entity, TestAppContext, VisualTestContext};

/// A main window that registered its native close path as the app opens one.
fn main_window(cx: &mut TestAppContext) -> (Entity<HerdrWindow>, &mut VisualTestContext) {
    cx.add_window_view(|window, cx| {
        install(window, cx);
        fixture_window(window, cx)
    })
}

fn close_command(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.command(Command::CloseWindow, window, cx);
        });
    });
    cx.run_until_parked();
}

#[gpui::test]
fn platform_close_asks_and_cancel_keeps_the_window(cx: &mut TestAppContext) {
    let (_view, cx) = main_window(cx);
    assert!(
        !cx.simulate_close(),
        "the platform close waits for an answer"
    );
    cx.run_until_parked();
    assert_eq!(
        cx.pending_prompt(),
        Some((MESSAGE.to_owned(), DETAIL.to_owned()))
    );

    // A repeat request while the prompt is open neither stacks another prompt
    // nor closes the window behind it.
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert_eq!(
        cx.pending_prompt(),
        Some((MESSAGE.to_owned(), DETAIL.to_owned()))
    );
    cx.simulate_prompt_answer(CANCEL);
    cx.run_until_parked();
    assert!(
        !cx.has_pending_prompt(),
        "one prompt was open, so one answer clears it"
    );
    assert_eq!(cx.windows().len(), 1, "cancel keeps the window");
}

#[gpui::test]
fn agreed_platform_close_removes_only_that_window(cx: &mut TestAppContext) {
    let (_view, cx) = main_window(cx);
    let other = cx.add_window(|window, cx| {
        install(window, cx);
        fixture_window(window, cx)
    });
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "the close asks first");
    cx.simulate_prompt_answer(CLOSE);
    cx.run_until_parked();
    assert_eq!(cx.windows(), [other.into()]);
}

#[gpui::test]
fn close_window_command_asks_and_a_cancel_does_not_block_a_later_close(cx: &mut TestAppContext) {
    let (view, cx) = main_window(cx);
    close_command(&view, cx);
    assert!(cx.has_pending_prompt(), "the command asks first");
    cx.simulate_prompt_answer(CANCEL);
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert_eq!(cx.windows().len(), 1);

    close_command(&view, cx);
    assert!(cx.has_pending_prompt(), "cancel left nothing behind");
    cx.simulate_prompt_answer(CLOSE);
    cx.run_until_parked();
    assert!(cx.windows().is_empty());
}
