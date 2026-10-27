#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    containers::{ContainerView, Containers, MAX_CONTAINERS, MAX_SLOTS, session_ref},
    control::protocol::{Caller, MAX_MESSAGE},
    endpoint::Endpoint,
    sidebar::layout_tests::fixture_window,
};
use gpui::{TestAppContext, WindowHandle};
use herdr_client::ConnectTarget;

fn call(op: ContainerOp) -> Call<ContainerOp> {
    Call {
        caller: Default::default(),
        op,
    }
}

fn code(response: &Response) -> Option<ErrorCode> {
    match response {
        Response::Error { code, .. } => Some(*code),
        _ => None,
    }
}

#[gpui::test]
fn every_operation_needs_a_window(cx: &mut TestAppContext) {
    cx.update(|cx| {
        for op in [
            ContainerOp::List,
            ContainerOp::New { title: None },
            ContainerOp::Focus { window: "x".into() },
            ContainerOp::Attach {
                session: "work".into(),
                device: None,
                window: None,
            },
            ContainerOp::Detach {
                session: "work".into(),
                device: None,
                window: None,
            },
            ContainerOp::Rename {
                window: "x".into(),
                title: "y".into(),
            },
            ContainerOp::Close { window: "x".into() },
        ] {
            let response = container(call(op), cx);
            assert_eq!(code(&response), Some(ErrorCode::NoWindow));
        }
    });
}

#[gpui::test]
fn malformed_attach_and_title_requests_are_refused_before_acting(cx: &mut TestAppContext) {
    cx.update(|cx| {
        for (session, device) in [
            ("", None),
            ("a\u{1b}b", None),
            ("work", Some("")),
            ("work", Some("x\n")),
            ("../escape", None),
        ] {
            let response = container(
                call(ContainerOp::Attach {
                    session: session.into(),
                    device: device.map(str::to_owned),
                    window: None,
                }),
                cx,
            );
            assert_eq!(
                code(&response),
                Some(ErrorCode::InvalidRequest),
                "{session:?}"
            );
        }
    });
}

#[gpui::test]
fn list_reports_windows_sessions_selection_and_status(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        view.endpoints.push(Endpoint::new(
            "ssh:box".into(),
            "Box".into(),
            ConnectTarget::Ssh {
                target: "box".into(),
                session: "default".into(),
            },
            true,
        ));
        view.list_all_endpoints();
        view.attach_session(session_ref("ssh:box", "work"), cx);
        view.new_container(Some("Second"), cx).unwrap();
        let containers = view.control_containers();
        assert_eq!(containers.len(), 2);
        assert_eq!(containers[1].title, "Second");
        assert!(containers[1].active && !containers[0].active);
        assert!(containers[1].sessions.is_empty());
        let first = &containers[0].sessions;
        assert!(
            first
                .iter()
                .any(|s| s.device == "ssh:box" && s.session == "work" && s.selected)
        );
        assert!(first.iter().all(|s| !s.status.is_empty()));
    });
}

#[gpui::test]
fn attach_rules_follow_the_catalog(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        assert_eq!(view.shown_device(), crate::endpoint::LOCAL);
        // A device the window does not know cannot be attached to.
        assert!(!view.can_attach(&session_ref("ssh:ghost", "work")));
        view.endpoints.push(Endpoint::new(
            "ssh:box".into(),
            "Box".into(),
            ConnectTarget::Ssh {
                target: "box".into(),
                session: "default".into(),
            },
            true,
        ));
        view.list_all_endpoints();
        assert!(view.can_attach(&session_ref("ssh:box", "work")));
        view.new_container(Some("Build"), cx).unwrap();
        view.attach_session(session_ref("ssh:box", "work"), cx);
        let containers = view.control_containers();
        assert_eq!(containers[1].sessions.len(), 1);
        let second = ContainerId::parse(&containers[1].id).unwrap();
        assert_eq!(view.containers.set().index_of(second), Some(1));
        assert!(matches!(find(view.containers.set(), "Build"), Ok(id) if id == second));
        assert!(find(view.containers.set(), "missing").is_err());
        let first = view.containers.containers()[0].id;
        view.switch_container(first, cx);
        assert!(view.control_containers()[0].active);
    });
}

fn box_device(view: &mut HerdrWindow) {
    view.endpoints.push(Endpoint::new(
        "ssh:box".into(),
        "Box".into(),
        ConnectTarget::Ssh {
            target: "box".into(),
            session: "default".into(),
        },
        true,
    ));
    view.list_all_endpoints();
}

/// A native window on the app's shared registry, as every real one is.
fn shared(cx: &mut TestAppContext) -> WindowHandle<HerdrWindow> {
    cx.update(|cx| {
        if !cx.has_global::<Containers>() {
            cx.set_global(Containers::default());
        }
    });
    cx.add_window(|window, cx| {
        let mut view = fixture_window(window, cx);
        let base = view.endpoints[0].session_ref();
        view.containers = ContainerView::open(&base, cx);
        view
    })
}

fn addressed(window: &WindowHandle<HerdrWindow>) -> Caller {
    Caller {
        window: Some(window.window_id().as_u64()),
        ..Caller::default()
    }
}

fn focus(window: &WindowHandle<HerdrWindow>, reference: &str, cx: &mut TestAppContext) -> Response {
    cx.update(|cx| {
        container(
            Call {
                caller: addressed(window),
                op: ContainerOp::Focus {
                    window: reference.into(),
                },
            },
            cx,
        )
    })
}

fn message(response: &Response) -> &str {
    match response {
        Response::Error { message, .. } | Response::Done { message } => message,
        _ => "",
    }
}

#[gpui::test]
fn attach_to_a_full_window_fails_for_the_shown_and_for_another_window(cx: &mut TestAppContext) {
    let window = cx.add_window(fixture_window);
    let other = window
        .update(cx, |view, _, cx| {
            box_device(view);
            let shown = view.containers.shown_id();
            let other = view
                .edit_containers(cx, |set| set.create(Some("Other")))
                .unwrap();
            view.edit_containers(cx, |set| {
                for index in 0..MAX_SLOTS {
                    for id in [shown, other] {
                        // The shown Window may already hold a member; the attaches
                        // past its limit fail and are fine here.
                        let _ = set.attach(id, session_ref("ssh:box", format!("full{index}")));
                    }
                }
                Ok(())
            })
            .unwrap();
            other
        })
        .unwrap();
    cx.update(|cx| {
        for target in [None, Some(other.to_string())] {
            let response = container(
                Call {
                    caller: addressed(&window),
                    op: ContainerOp::Attach {
                        session: "overflow".into(),
                        device: Some("ssh:box".into()),
                        window: target,
                    },
                },
                cx,
            );
            assert_eq!(code(&response), Some(ErrorCode::Failed), "{response:?}");
            assert!(message(&response).contains("64"), "{response:?}");
        }
    });
    window
        .update(cx, |view, _, _| {
            let overflow = session_ref("ssh:box", "overflow");
            assert!(
                view.containers
                    .set()
                    .containers()
                    .iter()
                    .all(|c| !c.members.contains(&overflow))
            );
        })
        .unwrap();
}

#[gpui::test]
fn focus_finds_a_window_another_native_window_just_created(cx: &mut TestAppContext) {
    let first = shared(cx);
    let second = shared(cx);
    let fresh = first
        .update(cx, |view, _, cx| {
            view.new_container(Some("Fresh"), cx).unwrap()
        })
        .unwrap();
    // `second` has not polled, so its mirror does not list "Fresh" yet.
    let response = focus(&second, "Fresh", cx);
    assert_eq!(response, Response::done(format!("Showing window {fresh}")));
    let shown = second.update(cx, |view, _, _| view.containers.shown_id());
    assert_eq!(shown.unwrap(), fresh);
}

#[gpui::test]
fn focus_of_a_window_closed_elsewhere_is_not_found(cx: &mut TestAppContext) {
    let first = shared(cx);
    let second = shared(cx);
    let gone = first
        .update(cx, |view, _, cx| {
            view.new_container(Some("Gone"), cx).unwrap()
        })
        .unwrap();
    second
        .update(cx, |view, _, cx| view.sync_containers(cx))
        .unwrap();
    first
        .update(cx, |view, _, cx| view.close_container(gone, cx).unwrap())
        .unwrap();
    // `second` still mirrors the closed Window until it polls.
    for reference in [gone.to_string(), "Gone".into()] {
        let response = focus(&second, &reference, cx);
        assert_eq!(code(&response), Some(ErrorCode::NotFound), "{reference}");
    }
    let shown = second.update(cx, |view, _, _| view.containers.shown_id());
    assert_ne!(shown.unwrap(), gone);
}

fn worst_case_windows() -> Vec<ContainerInfo> {
    (0..MAX_CONTAINERS)
        .map(|window| ContainerInfo {
            id: ContainerId::new().to_string(),
            title: "t".repeat(64),
            grouping: "project".into(),
            active: window == 0,
            sessions: (0..MAX_SLOTS)
                .map(|slot| SlotInfo {
                    device: "d".repeat(128),
                    session: format!("{slot:04}{}", "s".repeat(1020)),
                    selected: slot == 0,
                    status: "not connected".into(),
                })
                .collect(),
            sessions_total: MAX_SLOTS,
        })
        .collect()
}

fn answer_size(containers: &[ContainerInfo]) -> usize {
    let response = Response::Container {
        reply: Reply::Containers {
            containers: containers.to_vec(),
        },
    };
    serde_json::to_vec(&response).unwrap().len()
}

#[test]
fn the_worst_case_list_is_cut_to_the_budget_and_says_so() {
    let mut windows = worst_case_windows();
    // Measured: 32 Windows of 64 longest sessions would be refused by the
    // socket (MAX_MESSAGE) without this cut.
    assert!(answer_size(&windows) >= MAX_MESSAGE);
    fit_budget(&mut windows, LIST_BUDGET);
    assert!(answer_size(&windows) < MAX_MESSAGE);
    assert!(answer_size(&windows) <= LIST_BUDGET + 1024);
    for window in &windows {
        assert_eq!(window.sessions_total, MAX_SLOTS);
        assert!(!window.sessions.is_empty(), "every Window keeps a share");
        assert!(window.sessions.len() < MAX_SLOTS);
        assert!(window.sessions[0].selected && window.sessions[0].session.starts_with("0000"));
    }
}

#[test]
fn a_list_within_the_budget_is_left_whole() {
    let mut windows: Vec<ContainerInfo> = worst_case_windows().into_iter().take(2).collect();
    for window in &mut windows {
        window.sessions.truncate(3);
        window.sessions_total = 3;
    }
    let before = windows.clone();
    fit_budget(&mut windows, LIST_BUDGET);
    assert_eq!(windows, before);
}

#[test]
fn small_windows_do_not_cost_a_full_window_its_sessions() {
    // One full Window and 31 empty ones is far below the budget in total, so
    // nothing may be cut.
    let mut windows = worst_case_windows();
    for window in windows.iter_mut().skip(1) {
        window.sessions.clear();
        window.sessions_total = 0;
    }
    windows[0].sessions.truncate(40);
    windows[0].sessions_total = 40;
    assert!(answer_size(&windows) < LIST_BUDGET);
    let before = windows.clone();
    fit_budget(&mut windows, LIST_BUDGET);
    assert_eq!(windows, before);
}

#[test]
fn an_over_budget_list_gives_unused_shares_to_the_windows_still_cut() {
    // Three full Windows exceed the budget; the 29 empty ones use almost
    // nothing, so each full Window keeps about a third of it, not 1/32.
    let mut windows = worst_case_windows();
    for window in windows.iter_mut().skip(3) {
        window.sessions.clear();
        window.sessions_total = 0;
    }
    assert!(answer_size(&windows) > LIST_BUDGET);
    fit_budget(&mut windows, LIST_BUDGET);
    assert!(answer_size(&windows) <= LIST_BUDGET + 1024);
    for window in &windows[..3] {
        assert!(window.sessions.len() < MAX_SLOTS);
        assert!(window.sessions.len() >= 30, "{}", window.sessions.len());
        assert_eq!(window.sessions_total, MAX_SLOTS);
    }
}
