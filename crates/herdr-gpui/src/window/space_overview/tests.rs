//! Space Overview behavior: one overlay from either entry point, a selection
//! that is an identity rather than an index, deterministic keys, dismissal
//! that hands the keyboard back, and input that never reaches the pane below.
#![allow(clippy::unwrap_used)]

use super::columns;
use crate::{
    HerdrWindow,
    containers::{Container, ContainerId, ContainerSet, Grouping, session_ref},
    controls::Command,
    sidebar::layout_tests::{fixture_window, full_draw},
    window::MockPeer,
};
use gpui::{
    Entity, EntityInputHandler, KeyDownEvent, Keystroke, Modifiers, TestAppContext,
    VisualTestContext, point, px, size,
};
use herdr_client::protocol::{ClientMessage, ClientPaneInputEvent};

/// Fixed ids, so a card's debug selector is a constant: `debug_bounds` wants
/// `'static` text, and a test must not leak a string per Space.
const A: &str = "00000000-0000-4000-8000-00000000000a";
const B: &str = "00000000-0000-4000-8000-00000000000b";
const C: &str = "00000000-0000-4000-8000-00000000000c";
const D: &str = "00000000-0000-4000-8000-00000000000d";
const E: &str = "00000000-0000-4000-8000-00000000000e";
const CARD_A: &str = "space-overview-card-00000000-0000-4000-8000-00000000000a";
const CARD_B: &str = "space-overview-card-00000000-0000-4000-8000-00000000000b";
const CARD_C: &str = "space-overview-card-00000000-0000-4000-8000-00000000000c";
const OVERLAY: &str = "space-overview-overlay";
const PANEL: &str = "space-overview-panel";
const ENTRY: &str = "container-space-overview";

fn id(text: &str) -> ContainerId {
    ContainerId::parse(text).unwrap()
}

fn ids() -> Vec<ContainerId> {
    [A, B, C, D, E].iter().map(|text| id(text)).collect()
}

fn open(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> (Entity<HerdrWindow>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(width), px(height)));
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
}

/// Replaces the fixture's Space with five fixed empty ones, so cards are
/// addressable by constant selector. `extra` sessions are listed in the first.
fn fixed(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, extra: usize) {
    view.update(cx, |view, _| {
        let mut set = ContainerSet::validated(
            ids()
                .into_iter()
                .enumerate()
                .map(|(index, id)| Container {
                    id,
                    title: format!("Space {}", index + 1),
                    members: Vec::new(),
                    selected: None,
                    grouping: Grouping::None,
                })
                .collect(),
        )
        .unwrap();
        for index in 0..extra {
            set.attach(
                id(A),
                session_ref(crate::endpoint::LOCAL, format!("heavy-{index}")),
            )
            .unwrap();
        }
        view.containers.mirror(set, 1);
        assert_eq!(view.containers.containers().len(), 5);
        assert_eq!(view.containers.shown_id(), id(A));
    });
}

fn shown(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> ContainerId {
    view.read_with(cx, |view, _| view.containers.shown_id())
}

fn selected(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> Option<ContainerId> {
    view.read_with(cx, |view, _| {
        view.space_overview
            .as_ref()
            .and_then(|overview| overview.selected)
    })
}

fn overview_open(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.space_overview.is_some())
}

/// Whether the window's own focus handle holds the keyboard, which every
/// dismissal hands back.
fn focused(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.read(cx).focus.is_focused(window))
}

fn toggle(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| {
        view.command(Command::SpaceOverview, window, cx);
    });
}

fn show_overview(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.show_space_overview(window, cx));
}

/// The next input typed into the pane, skipping the resize and focus traffic
/// a drawn window sends on its own.
fn next_pane_input(peer: &mut MockPeer) -> ClientMessage {
    loop {
        let message = peer.receive();
        if matches!(message, ClientMessage::ClientShellPaneInput { .. }) {
            return message;
        }
    }
}

#[test]
fn the_overview_is_three_columns_only_from_900_points() {
    assert_eq!(columns(599.), 1);
    assert_eq!(columns(600.), 2);
    assert_eq!(columns(899.), 2);
    assert_eq!(columns(900.), 3);
}

#[gpui::test]
fn the_strip_entry_and_the_command_open_the_same_overlay(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, 1000., 600.);
    fixed(&view, cx, 0);
    draw(cx);
    let current = shown(&view, cx);
    let entry = cx.debug_bounds(ENTRY).unwrap().center();
    cx.simulate_click(entry, Modifiers::default());
    draw(cx);
    let from_entry = cx.debug_bounds(PANEL).unwrap();
    assert_eq!(
        selected(&view, cx),
        Some(current),
        "the shown Space starts selected"
    );
    assert!(cx.debug_bounds(CARD_A).is_some());
    // Click the backdrop, outside the measured panel.
    cx.simulate_click(
        point(from_entry.left() / 2., from_entry.center().y),
        Modifiers::default(),
    );
    draw(cx);
    assert!(!overview_open(&view, cx));
    // The command opens the very same overlay, in the same place.
    toggle(&view, cx);
    draw(cx);
    let from_command = cx.debug_bounds(PANEL).unwrap();
    assert_eq!(
        from_command, from_entry,
        "one overlay, whichever entry opened it"
    );
    assert_eq!(selected(&view, cx), Some(current));
    // And the same command toggles it shut.
    toggle(&view, cx);
    draw(cx);
    assert!(!overview_open(&view, cx));
    assert!(cx.debug_bounds(OVERLAY).is_none());
}

#[gpui::test]
fn opening_selects_the_shown_space_by_identity_not_by_position(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, 1000., 600.);
    fixed(&view, cx, 0);
    view.update(cx, |view, cx| {
        // Another native window moves the shown Space to the end of the registry.
        view.edit_containers(cx, |set| set.move_container(id(A), 4))
            .unwrap();
    });
    assert_eq!(
        view.read_with(cx, |view, _| view.containers.active_index()),
        4
    );
    show_overview(&view, cx);
    draw(cx);
    assert_eq!(
        selected(&view, cx),
        Some(id(A)),
        "the selection is the shown Space, not the card at its old index"
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.containers.containers()[0].id),
        id(B),
        "another Space is the first card"
    );
    assert!(cx.debug_bounds(CARD_A).is_some());
}

#[gpui::test]
fn arrows_step_one_column_at_640_and_clamp_and_enter_switches_then_escape_returns_focus(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, 640., 360.);
    fixed(&view, cx, 0);
    let all = ids();
    show_overview(&view, cx);
    draw(cx);
    let expect = |cx: &mut VisualTestContext, index: usize| {
        assert_eq!(
            selected(&view, cx),
            Some(all[index]),
            "selection at {index}"
        );
    };
    expect(cx, 0);
    cx.simulate_keystrokes("right right");
    expect(cx, 2);
    cx.simulate_keystrokes("down");
    expect(cx, 4); // one row down is two columns
    cx.simulate_keystrokes("right");
    expect(cx, 4); // clamped at the last Space
    cx.simulate_keystrokes("up");
    expect(cx, 2);
    cx.simulate_keystrokes("left");
    expect(cx, 1);
    cx.simulate_keystrokes("up");
    expect(cx, 0); // a row up from the first row saturates
    cx.simulate_keystrokes("up");
    expect(cx, 0);
    cx.simulate_keystrokes("right right enter");
    assert_eq!(
        shown(&view, cx),
        id(C),
        "Enter switched to the selected Space"
    );
    assert!(!overview_open(&view, cx));
    assert!(focused(&view, cx), "dismissal returns the keyboard");
    // Escape dismisses from wherever the selection is, switching nothing.
    show_overview(&view, cx);
    draw(cx);
    assert!(!focused(&view, cx), "the overlay owns the keyboard");
    cx.simulate_keystrokes("right");
    expect(cx, 3);
    cx.simulate_keystrokes("escape");
    assert!(!overview_open(&view, cx));
    assert_eq!(shown(&view, cx), id(C), "Escape switches nothing");
    assert!(focused(&view, cx));
}

#[gpui::test]
fn a_card_click_switches_and_returns_focus_while_the_backdrop_only_dismisses(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, 1000., 600.);
    fixed(&view, cx, 0);
    show_overview(&view, cx);
    draw(cx);
    let target = cx.debug_bounds(CARD_C).unwrap().center();
    cx.simulate_click(target, Modifiers::default());
    draw(cx);
    assert_eq!(shown(&view, cx), id(C));
    assert!(!overview_open(&view, cx));
    assert!(focused(&view, cx));
    // A click on the overlay outside the panel dismisses without switching.
    show_overview(&view, cx);
    draw(cx);
    let overlay = cx.debug_bounds(OVERLAY).unwrap();
    cx.simulate_click(
        point(overlay.left() + px(4.), overlay.top() + px(4.)),
        Modifiers::default(),
    );
    draw(cx);
    assert!(!overview_open(&view, cx));
    assert_eq!(shown(&view, cx), id(C));
    assert!(focused(&view, cx));
}

#[gpui::test]
fn a_space_deleted_elsewhere_does_not_switch_to_its_replacement(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, 1000., 600.);
    fixed(&view, cx, 0);
    show_overview(&view, cx);
    draw(cx);
    cx.simulate_keystrokes("right");
    assert_eq!(selected(&view, cx), Some(id(B)));
    // Another native window closes the selected empty Space.
    view.update(cx, |view, cx| {
        view.edit_containers(cx, |set| set.close(id(B))).unwrap();
    });
    draw(cx);
    assert!(
        cx.debug_bounds(CARD_B).is_none(),
        "the card went with the Space"
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.containers.containers()[1].id),
        id(C),
        "another Space took the old position"
    );
    cx.simulate_keystrokes("enter");
    assert!(!overview_open(&view, cx));
    assert_eq!(
        shown(&view, cx),
        id(A),
        "Enter did not follow the replacement"
    );
    // A click on the same vanished target does not switch either.
    show_overview(&view, cx);
    draw(cx);
    view.update_in(cx, |view, window, cx| {
        view.activate_overview_space(id(B), window, cx);
    });
    draw(cx);
    assert!(!overview_open(&view, cx));
    assert_eq!(shown(&view, cx), id(A));
    // A target that still exists does switch.
    show_overview(&view, cx);
    view.update_in(cx, |view, window, cx| {
        view.activate_overview_space(id(E), window, cx);
    });
    assert_eq!(shown(&view, cx), id(E));
}

#[gpui::test]
fn the_overview_holds_the_keyboard_away_from_the_pane(cx: &mut TestAppContext) {
    let mut peer = MockPeer::new();
    let (view, cx) = cx.add_window_view(fixture_window);
    draw(cx);
    cx.update(|_, cx| view.update(cx, |view, cx| peer.prepare(view, cx)));
    assert!(view.read_with(cx, |view, _| view.input_ready()));
    // Opening drops a composition in progress, and a commit or mark callback
    // from the input method after that does nothing at all.
    view.update(cx, |view, _| {
        view.marked = "a".into();
        view.marked_selection = Some(1..1);
    });
    toggle(&view, cx);
    draw(cx);
    assert!(
        !view.read_with(cx, |view, _| view.input_ready()),
        "the overlay is not terminal input"
    );
    view.update_in(cx, |view, window, cx| {
        assert!(view.marked.is_empty(), "opening dropped the composition");
        assert_eq!(view.marked_selection, None);
        view.replace_text_in_range(None, "x", window, cx);
        assert!(view.marked.is_empty(), "a commit composes nothing");
        assert_eq!(view.marked_selection, None);
        view.replace_and_mark_text_in_range(None, "y", Some(0..1), window, cx);
        assert!(view.marked.is_empty(), "a mark callback composes nothing");
        view.paste(cx);
        let key = KeyDownEvent {
            keystroke: Keystroke::parse("a").unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        view.key_down(&key, window, cx);
    });
    // Another command is blocked while the overlay owns the window.
    let sidebar = view.read_with(cx, |view, _| view.sidebar_visible);
    view.update_in(cx, |view, window, cx| {
        view.command(Command::ToggleSidebar, window, cx);
    });
    assert_eq!(view.read_with(cx, |view, _| view.sidebar_visible), sidebar);
    // Only the sentinel reaches the pane, so none of the above sent anything.
    let sentinel = ClientPaneInputEvent::TextCommit("sentinel".into());
    peer.client
        .handle
        .send_input("boot-v1", "w1:p1", [sentinel.clone()])
        .unwrap();
    assert_eq!(
        next_pane_input(&mut peer),
        ClientMessage::ClientShellPaneInput {
            pane_id: "w1:p1".into(),
            events: vec![sentinel],
        }
    );
    // Dismissal restores keyboard ownership; a resized pane still waits for its surface.
    toggle(&view, cx);
    assert!(focused(&view, cx));
}

#[gpui::test]
fn a_page_or_the_native_close_prompt_refuses_the_overview(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, 1000., 600.);
    let current = shown(&view, cx);
    // A workspace page owns the window.
    assert!(view.update_in(cx, |view, window, cx| view.open_menu(window, cx)));
    toggle(&view, cx);
    draw(cx);
    assert!(!overview_open(&view, cx));
    assert!(view.read_with(cx, |view, _| view.menu.page.is_some()));
    view.update_in(cx, |view, window, cx| view.dismiss_menu(window, cx));
    draw(cx);
    // The native close confirmation owns it too, and it closes nothing.
    view.update_in(cx, |_, window, cx| {
        assert!(crate::native_close::request_close(window, cx));
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    toggle(&view, cx);
    assert!(!overview_open(&view, cx));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, cx| cx.windows().len()),
        1,
        "Cancel closes nothing"
    );
    toggle(&view, cx);
    draw(cx);
    assert!(overview_open(&view, cx));
    // The overlay in turn refuses a page while it owns the window.
    assert!(!view.update_in(cx, |view, window, cx| view.open_menu(window, cx)));
    assert!(overview_open(&view, cx));
    // A Space's own menu is refused the same way.
    view.update_in(cx, |view, window, cx| {
        view.open_container_menu(current, point(px(0.), px(0.)), window, cx);
    });
    toggle(&view, cx);
    assert!(!overview_open(&view, cx));
    assert!(view.read_with(cx, |view, _| view.container_menu.is_some()));
}

#[gpui::test]
fn escape_from_an_empty_space_returns_focus_so_the_shortcut_reopens(cx: &mut TestAppContext) {
    // The empty Space draws its own root instead of a terminal, so the focus
    // handle must be tracked there too or the shortcut reaches nothing.
    let (view, cx) = cx.add_window_view(|window, cx| {
        crate::bind_keys(cx);
        let mut view = fixture_window(window, cx);
        view.containers.mirror(
            ContainerSet::validated(vec![Container {
                id: id(A),
                title: "Space 1".into(),
                members: Vec::new(),
                selected: None,
                grouping: Grouping::None,
            }])
            .unwrap(),
            1,
        );
        view
    });
    cx.simulate_resize(size(px(1000.), px(600.)));
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.container_is_empty()));
    assert!(cx.debug_bounds("container-empty").is_some());
    let shortcut = view.read_with(cx, |view, _| {
        view.keymap().primary(Command::SpaceOverview).to_owned()
    });
    assert!(!shortcut.is_empty(), "the command has a shortcut");
    // Escape hands the keyboard back; the shortcut opens the overlay again.
    show_overview(&view, cx);
    draw(cx);
    cx.simulate_keystrokes("escape");
    assert!(!overview_open(&view, cx));
    assert!(focused(&view, cx));
    cx.simulate_keystrokes(&shortcut);
    draw(cx);
    assert!(
        overview_open(&view, cx),
        "the shortcut reopened the overlay from the empty Space"
    );
    assert!(cx.debug_bounds(PANEL).is_some());
    // A card click into the same empty Space dismisses it and hands focus
    // back the same way.
    let target = cx.debug_bounds(CARD_A).unwrap().center();
    cx.simulate_click(target, Modifiers::default());
    draw(cx);
    assert!(!overview_open(&view, cx));
    assert!(focused(&view, cx));
    cx.simulate_keystrokes(&shortcut);
    draw(cx);
    assert!(overview_open(&view, cx));
    cx.simulate_keystrokes(&shortcut);
    assert!(!overview_open(&view, cx));
}

#[gpui::test]
fn a_640_wide_window_bounds_the_panel_and_cards_however_many_sessions(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, 640., 360.);
    // One Space lists far more sessions than fit: the card caps its list
    // rather than growing a preview.
    fixed(&view, cx, 40);
    show_overview(&view, cx);
    draw(cx);
    let overlay = cx.debug_bounds(OVERLAY).unwrap();
    assert!(
        f32::from(overlay.right()) <= 640. && f32::from(overlay.bottom()) <= 360.,
        "the overlay stays inside the window: {overlay:?}"
    );
    let panel = cx.debug_bounds(PANEL).unwrap();
    let viewport = cx.update(|window, _| window.viewport_size());
    let expected = (f32::from(viewport.width) - 32.).min(1080.);
    assert!(
        (f32::from(panel.size.width) - expected).abs() < 0.5,
        "the panel is viewport - 32: {:?} vs {expected}",
        panel.size.width
    );
    assert!(
        f32::from(panel.top()) >= 0. && f32::from(panel.bottom()) <= 360.,
        "the panel fits the window: {panel:?}"
    );
    assert!(
        panel.left() >= overlay.left() && panel.right() <= overlay.right(),
        "the panel is centered inside the overlay"
    );
    let heavy = cx.debug_bounds(CARD_A).unwrap();
    assert!(
        f32::from(heavy.size.height) <= 300.,
        "forty sessions stay in a capped list: {:?}",
        heavy.size.height
    );
    let second = cx.debug_bounds(CARD_B).unwrap();
    assert_eq!(heavy.top(), second.top(), "two cards share the first row");
    assert_eq!(heavy.size.width, second.size.width, "columns are equal");
    assert!(heavy.right() <= second.left(), "columns must not overlap");
    assert!(
        cx.debug_bounds(ENTRY).is_some(),
        "the strip entry stays reachable at 640"
    );
}
