//! The footer's device choice is the Sessions manager's starting filter: copied
//! at every open, then independent of the footer and of every other filter.
use super::*;
use herdr_client::RemoteSession;

/// Two local sessions and a saved Build device reporting two of its own, with
/// the footer still on All Devices and the list open.
fn fixture(cx: &mut TestAppContext) -> (Entity<crate::HerdrWindow>, &mut VisualTestContext) {
    let (view, cx) = open_list(
        cx,
        vec![
            session("default", SessionState::Running),
            session("work", SessionState::Stopped),
        ],
        development("default"),
    );
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.endpoints
                .push(remote("ssh:build", "Build", "build.invalid", "remote-run"));
            view.list_all_endpoints();
            view.sessions.devices.answers.insert(
                "ssh:build".into(),
                crate::sessions::DeviceScan::Sessions(vec![
                    RemoteSession {
                        name: "remote-run".into(),
                        running: true,
                        workspaces: Vec::new(),
                    },
                    RemoteSession {
                        name: "remote-stop".into(),
                        running: false,
                        workspaces: Vec::new(),
                    },
                ]),
            );
        })
    });
    draw(cx);
    (view, cx)
}

fn choose_footer_device(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("escape");
    draw(cx);
    click(cx, "device-picker");
    draw(cx);
    // All Devices, Local, Build, Add Device….
    cx.simulate_keystrokes("down down enter");
    draw(cx);
}

fn reopen(cx: &mut VisualTestContext) {
    click(cx, "device-sessions");
    draw(cx);
}

#[gpui::test]
fn selecting_a_footer_device_opens_the_manager_on_that_device(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).session_device_filter()),
        None,
        "All Devices starts unfiltered"
    );
    assert_eq!(names(&view, cx).len(), 4);
    choose_footer_device(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.device_filter.as_deref(), Some("ssh:build"));
        assert_eq!(view.menu.page, None, "choosing a device closes the picker");
        assert_eq!(
            view.endpoints[view.selected_endpoint].device_id(),
            "ssh:build"
        );
    });
    // The picker's own dialog is gone, so the footer's icon is the way in.
    reopen(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.session_device_filter().as_deref(), Some("ssh:build"));
        assert_eq!(
            view.session_query(),
            "",
            "the copied filter is not a search"
        );
        assert_eq!(view.menu.page, Some(Page::Sessions));
    });
    assert_eq!(
        names(&view, cx),
        ["ssh:build:remote-run", "ssh:build:remote-stop"]
    );
}

#[gpui::test]
fn the_manager_changes_its_own_device_without_touching_the_footer(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    choose_footer_device(cx);
    reopen(cx);
    assert_eq!(
        names(&view, cx),
        ["ssh:build:remote-run", "ssh:build:remote-stop"]
    );
    click(cx, "sessions-facet-Device");
    draw(cx);
    // All devices, this machine, Build.
    click(cx, "sessions-choice-Device-1");
    draw(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.session_device_filter().as_deref(), Some("local"));
        assert_eq!(
            view.device_filter.as_deref(),
            Some("ssh:build"),
            "the footer keeps its own choice"
        );
        assert_eq!(
            view.endpoints[view.selected_endpoint].device_id(),
            "ssh:build"
        );
    });
    let local = names(&view, cx);
    assert_eq!(local.len(), 2);
    assert!(local.iter().all(|name| name.starts_with("local:")));
    // Back to All devices inside the manager: the footer stays where it was.
    click(cx, "sessions-facet-Device");
    draw(cx);
    click(cx, "sessions-choice-Device-0");
    draw(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.session_device_filter(), None);
        assert_eq!(view.device_filter.as_deref(), Some("ssh:build"));
    });
    assert_eq!(names(&view, cx).len(), 4);
}

#[gpui::test]
fn every_open_copies_the_footer_again(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    choose_footer_device(cx);
    reopen(cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).session_device_filter()),
        Some("ssh:build".into())
    );
    // Narrow the open manager to this machine, then close it.
    click(cx, "sessions-facet-Device");
    draw(cx);
    click(cx, "sessions-choice-Device-1");
    draw(cx);
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).session_device_filter().as_deref(),
            Some("local")
        );
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    reopen(cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).session_device_filter()),
        Some("ssh:build".into()),
        "a fresh open starts from the footer, not the last manager"
    );
    assert_eq!(
        names(&view, cx),
        ["ssh:build:remote-run", "ssh:build:remote-stop"]
    );
    // All Devices in the footer opens the manager unfiltered.
    cx.simulate_keystrokes("escape");
    draw(cx);
    click(cx, "device-picker");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.update(|_, cx| assert!(view.read(cx).device_filter.is_none()));
    reopen(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.session_device_filter(), None);
        assert_eq!(view.menu.page, Some(Page::Sessions));
    });
    assert_eq!(names(&view, cx).len(), 4);
}

#[gpui::test]
fn the_footer_device_is_only_the_starting_filter(cx: &mut TestAppContext) {
    let (view, cx) = fixture(cx);
    choose_footer_device(cx);
    reopen(cx);
    // A run-state filter and a search still apply on top of the copied device.
    click(cx, "sessions-filter-Running");
    draw(cx);
    assert_eq!(names(&view, cx), ["ssh:build:remote-run"]);
    cx.simulate_keystrokes("s t o p");
    draw(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.session_query(), "stop");
        assert_eq!(view.session_device_filter().as_deref(), Some("ssh:build"));
        assert_eq!(view.device_filter.as_deref(), Some("ssh:build"));
    });
    // The state filter still hides a stopped session of that device.
    assert!(names(&view, cx).is_empty());
    // Clearing the state filter leaves the device filter, and the footer, alone.
    click(cx, "sessions-filter-All");
    draw(cx);
    assert_eq!(names(&view, cx), ["ssh:build:remote-stop"]);
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(view.session_device_filter().as_deref(), Some("ssh:build"));
        assert_eq!(view.device_filter.as_deref(), Some("ssh:build"));
    });
    // The project facet narrows on its own axis: the device filter and the
    // footer both keep the choice they had.
    click(cx, "sessions-facet-Project");
    draw(cx);
    click(cx, "sessions-choice-Project-1");
    draw(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert_eq!(
            view.session_scope().facets.project,
            crate::menu::sessions::ProjectPick::Unassigned
        );
        assert_eq!(view.session_device_filter().as_deref(), Some("ssh:build"));
        assert_eq!(view.device_filter.as_deref(), Some("ssh:build"));
    });
}
