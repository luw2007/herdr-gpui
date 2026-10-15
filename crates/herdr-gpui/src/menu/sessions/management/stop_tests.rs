use super::{Edit, Target};
use crate::sidebar::layout_tests::fixture_window;
use crate::{
    endpoint::Endpoint,
    menu::{Page, sessions::Row},
    sessions::DeviceScan,
};
use gpui::{TestAppContext, point, px};
use herdr_client::{ConnectTarget, LocalSession, RemoteHost, RemoteSession, SessionState};

fn paused(endpoint: &Endpoint) -> bool {
    endpoint.status() == "detached"
}

fn local(name: &str) -> ConnectTarget {
    ConnectTarget::Session {
        name: name.into(),
        development: false,
    }
}

fn ssh(host: &str, session: &str) -> ConnectTarget {
    ConnectTarget::Ssh {
        target: host.into(),
        session: session.into(),
    }
}

fn session(name: &str, state: SessionState) -> LocalSession {
    LocalSession {
        name: name.into(),
        state,
        socket: local(name).socket_path().unwrap(),
        workspaces: Vec::new(),
    }
}

#[gpui::test]
fn stop_is_offered_for_running_sessions_only_and_never_for_default(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.endpoints[0].connection.target = local("default");
            view.sessions.entries = vec![
                session("default", SessionState::Running),
                session("busy", SessionState::Running),
                session("idle", SessionState::Stopped),
            ];
            let reason = |view: &crate::HerdrWindow, name: &str| {
                view.session_stop_reason(&Row::Local(name.into()))
            };
            assert!(reason(view, "default").is_some());
            assert!(reason(view, "idle").is_some());
            assert!(reason(view, "missing").is_some());
            assert!(reason(view, "busy").is_none());
            assert!(view.session_stop_reason(&Row::Add(Target::Local)).is_some());
        })
    });
}

#[gpui::test]
fn confirming_stop_keeps_the_list_and_cancel_returns_to_search(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints[0].connection.target = local("default");
            view.sessions.entries = vec![session("busy", SessionState::Running)];
            view.open_sessions(point(px(10.), px(440.)), window, cx);
            view.confirm_session_stop(Row::Local("busy".into()), Some(Target::Local), window, cx);
            assert!(matches!(view.menu.session_edit, Some(Edit::Stop { .. })));
            // Opening the confirmation changes nothing yet.
            assert_eq!(view.sessions.entries.len(), 1);
            assert!(view.sessions.entries[0].state.running());
            assert!(view.sessions.mutation.is_none());
            view.cancel_session_edit(window, cx);
            assert!(view.menu.session_edit.is_none());
            assert_eq!(view.menu.page, Some(Page::Sessions));
            let search = view.menu.sessions_view.as_ref().unwrap().search.clone();
            assert!(search.read(cx).focus.is_focused(window));
        })
    });
}

#[gpui::test]
fn a_changed_device_refuses_to_stop(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints.push(Endpoint::new(
                "ssh:test".into(),
                "Test".into(),
                ssh("old.invalid", "default"),
                true,
            ));
            view.list_all_endpoints();
            view.sessions.devices.answers.insert(
                "ssh:test".into(),
                DeviceScan::Sessions(vec![RemoteSession {
                    name: "busy".into(),
                    running: true,
                    workspaces: Vec::new(),
                }]),
            );
            let row = Row::Device {
                id: "ssh:test".into(),
                session: "busy".into(),
            };
            view.menu.session_edit = Some(Edit::Stop {
                row: row.clone(),
                target: Target::Device {
                    id: "ssh:test".into(),
                    host: RemoteHost::Ssh("old.invalid".into()),
                },
            });
            view.endpoints[1].connection.target = ssh("replacement.invalid", "default");
            view.submit_session_edit(window, cx);
            assert!(view.menu.error.as_ref().unwrap().contains("changed"));
            assert!(view.sessions.mutation.is_none());
            assert!(
                !paused(&view.endpoints[1]),
                "the replacement host must not be paused"
            );
            // Paint-time fence.
            view.menu.session_edit = None;
            view.confirm_session_stop(
                row,
                Some(Target::Device {
                    id: "ssh:test".into(),
                    host: RemoteHost::Ssh("old.invalid".into()),
                }),
                window,
                cx,
            );
            assert!(view.menu.session_edit.is_none());
            assert!(
                view.sessions
                    .mutation_error
                    .as_ref()
                    .unwrap()
                    .contains("changed")
            );
        })
    });
}

#[gpui::test]
fn pausing_fences_only_the_same_host_and_session(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.endpoints[0].connection.target = local("demo");
            for (id, host, session) in [
                ("ssh:one", "host.invalid", "demo"),
                ("ssh:two", "other.invalid", "demo"),
                ("ssh:three", "host.invalid", "other"),
            ] {
                view.endpoints.push(Endpoint::new(
                    id.into(),
                    id.into(),
                    ssh(host, session),
                    true,
                ));
                view.list_all_endpoints();
            }
            assert!(!view.pause_session_endpoints(&ssh("host.invalid", "demo")));
            assert!(paused(&view.endpoints[1]));
            assert_eq!(view.endpoints[1].status(), "detached");
            assert_eq!(
                view.endpoints[1].connection.inbox.lock().unwrap().status,
                crate::state::ConnectionStatus::Detached
            );
            // Same session name on another host, and another session on the
            // same host, keep running; the local `demo` is another target.
            assert!(!paused(&view.endpoints[2]));
            assert!(!paused(&view.endpoints[3]));
            assert!(!paused(&view.endpoints[0]));
            // Saved profile target is untouched.
            assert_eq!(
                view.endpoints[1].connection.target,
                ssh("host.invalid", "demo")
            );
            // The selected endpoint reports that it was paused.
            view.selected_endpoint = 0;
            assert!(view.pause_session_endpoints(&local("demo")));
            assert!(paused(&view.endpoints[0]));
            assert_eq!(view.endpoints[0].connection.target, local("demo"));
        })
    });
}

#[gpui::test]
fn stopping_pauses_the_same_target_in_every_window(cx: &mut TestAppContext) {
    let first = cx.add_window(fixture_window);
    let second = cx.add_window(fixture_window);
    for handle in [first, second] {
        handle
            .update(cx, |view, _, _| {
                view.endpoints[0].connection.target = local("demo");
            })
            .unwrap();
    }
    let third = cx.add_window(fixture_window);
    third
        .update(cx, |view, _, _| {
            view.endpoints[0].connection.target = local("elsewhere");
        })
        .unwrap();
    first
        .update(cx, |view, window, cx| {
            // The window mid-update is handled directly and never re-entered.
            view.pause_session_in_all_windows(
                &local("demo"),
                window.window_handle().window_id(),
                cx,
            );
            assert!(paused(&view.endpoints[0]));
        })
        .unwrap();
    second
        .update(cx, |view, _, _| assert!(paused(&view.endpoints[0])))
        .unwrap();
    third
        .update(cx, |view, _, _| assert!(!paused(&view.endpoints[0])))
        .unwrap();
}

#[gpui::test]
fn deleting_isolates_other_windows_and_retargets_only_this_one(cx: &mut TestAppContext) {
    let first = cx.add_window(fixture_window);
    let second = cx.add_window(fixture_window);
    for handle in [first, second] {
        handle
            .update(cx, |view, _, _| {
                view.endpoints[0].connection.target = local("demo");
            })
            .unwrap();
    }
    first
        .update(cx, |view, window, cx| {
            let target = local("demo");
            view.pause_session_in_other_windows(&target, window.window_handle().window_id(), cx);
            assert!(!paused(&view.endpoints[0]));
            assert!(view.retarget_session_for_deletion(&target));
            assert_eq!(view.endpoints[0].connection.target, local("default"));
        })
        .unwrap();
    second
        .update(cx, |view, _, _| assert!(paused(&view.endpoints[0])))
        .unwrap();
}

#[gpui::test]
fn a_replaced_host_is_not_marked_stopped_when_the_old_stop_finishes(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.endpoints.push(Endpoint::new(
                "ssh:test".into(),
                "Test".into(),
                ssh("old.invalid", "default"),
                true,
            ));
            view.list_all_endpoints();
            view.sessions.devices.answers.insert(
                "ssh:test".into(),
                DeviceScan::Sessions(vec![RemoteSession {
                    name: "busy".into(),
                    running: true,
                    workspaces: Vec::new(),
                }]),
            );
            let row = Row::Device {
                id: "ssh:test".into(),
                session: "busy".into(),
            };
            let captured = Target::Device {
                id: "ssh:test".into(),
                host: RemoteHost::Ssh("old.invalid".into()),
            };
            // The catalog gave the same id to another host while the worker ran.
            view.endpoints[1].connection.target = ssh("replacement.invalid", "default");
            view.finish_session_stop(true, None, &row, &captured);
            let Some(DeviceScan::Sessions(sessions)) =
                view.sessions.devices.answers.get("ssh:test")
            else {
                panic!("answer disappeared")
            };
            assert!(
                sessions[0].running,
                "the other host's session was not stopped"
            );
            assert!(view.sessions.mutation.is_none());
            assert!(!view.sessions.mutation_stop);
        })
    });
}
