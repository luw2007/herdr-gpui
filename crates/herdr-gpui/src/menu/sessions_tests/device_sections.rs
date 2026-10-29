//! Choosing a device in the manager selects the sections it keeps; it is not a
//! search that would hide that section's own scan status or its Add action.
//! Whatever the choice excluded — the other devices' notes, their Add actions,
//! this machine's scan error — stays out of the list. Narrowing it further by
//! text or run state keeps the old behaviour: no notes, no Add, no match line.
use super::*;
use crate::{
    menu::sessions::{AddTarget, Entry, Row},
    sessions::DeviceScan,
};
use herdr_client::RemoteSession;

const BUILD: &str = "ssh:build";
const PROD: &str = "ssh:prod";

/// A saved SSH device, as the catalog creates its endpoint. The host never
/// resolves, so no test here reaches a real machine.
fn device(id: &str, label: &str, host: &str, session: &str) -> crate::endpoint::Endpoint {
    crate::endpoint::Endpoint::new(
        id.into(),
        label.into(),
        ConnectTarget::Ssh {
            target: host.into(),
            session: session.into(),
        },
        true,
    )
}

/// What one saved device's session probe reported.
fn answer(sessions: &[(&str, bool)]) -> DeviceScan {
    DeviceScan::Sessions(
        sessions
            .iter()
            .map(|(name, running)| RemoteSession {
                name: (*name).into(),
                running: *running,
                workspaces: Vec::new(),
            })
            .collect(),
    )
}

/// A window on this machine with its list open. `chosen` is the footer's
/// device choice, copied into the manager as it opens. Saved devices and their
/// answers are added afterwards, so no probe is ever asked of a host.
fn open<'a>(
    cx: &'a mut TestAppContext,
    chosen: Option<&str>,
) -> (Entity<crate::HerdrWindow>, &'a mut VisualTestContext) {
    let filter = chosen.map(str::to_owned);
    let (view, cx) = cx.add_window_view(move |window, cx| {
        let mut view = fixture_window(window, cx);
        view.endpoints[0].connection.target = development("default");
        view.sessions.entries = Vec::new();
        view.device_filter = filter;
        view.containers = crate::containers::ContainerView::local(&view.endpoints[0].session_ref());
        view
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.run_until_parked();
    draw(cx);
    click(cx, "device-sessions");
    draw(cx);
    (view, cx)
}

/// Save `endpoint` below this machine and record what its probe found. `None`
/// leaves it waiting, as an outstanding scan does.
fn save(
    view: &Entity<crate::HerdrWindow>,
    cx: &mut VisualTestContext,
    endpoint: crate::endpoint::Endpoint,
    found: Option<DeviceScan>,
) {
    let id = endpoint.id.clone();
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.endpoints.push(endpoint);
            view.list_all_endpoints();
            if let Some(found) = found {
                view.sessions.devices.answers.insert(id, found);
            }
        })
    });
    draw(cx);
}

/// Painted notes and sessions; Add targets are checked separately as typed rows.
fn painted(view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        view.read(cx)
            .session_entries()
            .iter()
            .filter_map(|entry| {
                Some(match entry {
                    Entry::Header { selector, .. } => format!("header:{selector}"),
                    Entry::Muted { selector, text } => format!("note:{selector}={text}"),
                    Entry::Error(text) => format!("error:{text}"),
                    Entry::Row {
                        row: Row::Local(name),
                        ..
                    } => format!("local:{name}"),
                    Entry::Row {
                        row: Row::Device { id, session },
                        ..
                    } => format!("{id}:{session}"),
                    Entry::Row {
                        row: Row::Add(_), ..
                    } => return None,
                })
            })
            .collect()
    })
}

/// The text of the note carrying `selector`, if it is painted.
fn note<'a>(lines: &'a [String], selector: &str) -> Option<&'a str> {
    let prefix = format!("note:{selector}=");
    lines.iter().find_map(|line| line.strip_prefix(&prefix))
}

/// The typed Add targets, independent of their existing availability rules.
fn actions(view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext) -> Vec<AddTarget> {
    cx.update(|_, cx| {
        view.read(cx)
            .session_entries()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Row {
                    row: Row::Add(target),
                    ..
                } => Some(target.clone()),
                _ => None,
            })
            .collect()
    })
}

#[gpui::test]
fn a_chosen_device_keeps_its_status_saved_session_and_add(cx: &mut TestAppContext) {
    let cases = [
        (None, "Checking…"),
        (
            Some(DeviceScan::Failed("ssh timed out".to_owned())),
            "ssh timed out",
        ),
        (
            Some(DeviceScan::Sessions(Vec::new())),
            "No sessions on this device.",
        ),
    ];
    for (found, status) in cases {
        let (view, cx) = open(cx, Some(BUILD));
        save(
            &view,
            cx,
            device(BUILD, "Build", "build.invalid", "default"),
            found,
        );
        let lines = painted(&view, cx);
        assert_eq!(
            note(&lines, &format!("sessions-device-note-{BUILD}")),
            Some(status),
            "the chosen device still says what its scan found: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.as_str() == "ssh:build:default"),
            "its saved session stays selectable: {lines:?}"
        );
        let actions = actions(&view, cx);
        let host = device(BUILD, "Build", "build.invalid", "default")
            .connection
            .target
            .remote_host()
            .unwrap();
        assert_eq!(
            actions,
            [AddTarget::Device {
                id: BUILD.into(),
                host,
            }]
        );
    }
}

#[gpui::test]
fn a_chosen_local_section_keeps_its_status_and_add(cx: &mut TestAppContext) {
    let cases = [
        (true, None, Some("Checking…")),
        (
            false,
            Some("this machine's scan failed"),
            Some("No sessions on this machine."),
        ),
        (false, None, Some("No sessions on this machine.")),
    ];
    for (scanning, error, status) in cases {
        let (view, cx) = open(cx, Some(LOCAL));
        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                view.sessions.entries = Vec::new();
                view.sessions.scanning = scanning;
                view.sessions.error = error.map(str::to_owned);
            })
        });
        draw(cx);
        let lines = painted(&view, cx);
        assert!(
            lines
                .iter()
                .any(|line| line.as_str() == "header:sessions-local-header"),
            "the chosen section is painted: {lines:?}"
        );
        assert_eq!(
            note(&lines, "sessions-empty"),
            status,
            "the chosen section still says what its scan found: {lines:?}"
        );
        if let Some(error) = error {
            assert!(
                lines.iter().any(|line| line == &format!("error:{error}")),
                "its scan error stays with it: {lines:?}"
            );
        }
        assert_eq!(actions(&view, cx), [AddTarget::Local], "{lines:?}");
        assert!(
            !lines
                .iter()
                .any(|line| line.starts_with("note:sessions-no-devices")),
            "no note about the sections it excluded: {lines:?}"
        );
    }
}

#[gpui::test]
fn a_chosen_device_hides_the_sections_it_excluded(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, Some(BUILD));
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.sessions.entries = Vec::new();
            view.sessions.scanning = true;
            view.sessions.error = Some("this machine's scan failed".to_owned());
        })
    });
    save(
        &view,
        cx,
        device(BUILD, "Build", "build.invalid", "default"),
        Some(DeviceScan::Failed("ssh timed out".to_owned())),
    );
    save(
        &view,
        cx,
        device(PROD, "Prod", "prod.invalid", "main"),
        None,
    );
    let lines = painted(&view, cx);
    // The chosen device keeps its own trouble and its Add action.
    assert_eq!(
        note(&lines, &format!("sessions-device-note-{BUILD}")),
        Some("ssh timed out"),
        "{lines:?}"
    );
    let actions = actions(&view, cx);
    assert!(
        matches!(actions.as_slice(), [AddTarget::Device { id, .. }] if id == BUILD),
        "only the chosen device offers Add: {lines:?}"
    );
    // Nothing the choice excluded is painted.
    for hidden in [
        "header:sessions-local-header",
        "header:sessions-device-ssh:prod",
        "note:sessions-empty",
        "note:sessions-device-note-ssh:prod",
        "error:this machine's scan failed",
    ] {
        assert!(
            !lines.iter().any(|line| line.starts_with(hidden)),
            "{hidden} leaked into the list: {lines:?}"
        );
    }
}

/// The lines for a chosen device that has left the catalog.
fn unlisted_device_lines(cx: &mut TestAppContext) -> Vec<String> {
    let (view, cx) = open(cx, Some("ssh:gone"));
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.sessions.entries = vec![session("default", SessionState::Running)];
        })
    });
    draw(cx);
    painted(&view, cx)
}

/// The lines for a text query that misses on the chosen device.
fn query_miss_lines(cx: &mut TestAppContext) -> Vec<String> {
    let (view, cx) = open(cx, Some(BUILD));
    save(
        &view,
        cx,
        device(BUILD, "Build", "build.invalid", "default"),
        Some(answer(&[("agents", true)])),
    );
    cx.simulate_keystrokes("z z z");
    cx.run_until_parked();
    draw(cx);
    painted(&view, cx)
}

/// The lines for a run-state filter that misses on the chosen device: its
/// saved session is unreachable and the one it reported is stopped.
fn state_miss_lines(cx: &mut TestAppContext) -> Vec<String> {
    let (view, cx) = open(cx, Some(BUILD));
    save(
        &view,
        cx,
        device(BUILD, "Build", "build.invalid", "default"),
        Some(answer(&[("agents", false)])),
    );
    click(cx, "sessions-filter-Running");
    draw(cx);
    painted(&view, cx)
}

/// The list holds nothing but the no-match line.
fn only_no_matches(lines: &[String], why: &str) {
    assert_eq!(lines.len(), 1, "{why}: {lines:?}");
    assert!(
        lines[0].starts_with("note:sessions-no-matches"),
        "{why}: {lines:?}"
    );
}

#[gpui::test]
fn a_chosen_device_with_nothing_to_show_still_says_no_matches(cx: &mut TestAppContext) {
    // The chosen device is no longer in the catalog: no section can keep it.
    only_no_matches(
        &unlisted_device_lines(cx),
        "a chosen device that is gone has nothing to list",
    );
    // Text is still a search: it hides the chosen device's status and Add
    // action, and reports that nothing matched.
    only_no_matches(
        &query_miss_lines(cx),
        "a text miss has no status or Add action",
    );
    only_no_matches(
        &state_miss_lines(cx),
        "a run-state miss has no status or Add action",
    );
}
