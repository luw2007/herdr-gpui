#![allow(clippy::unwrap_used)]
use super::*;
use crate::{endpoint::Endpoint, sidebar::layout_tests::fixture_window};
use gpui::TestAppContext;
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
            ContainerOp::Focus { id: 1 },
            ContainerOp::Attach {
                session: "work".into(),
                device: None,
            },
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
        view.attach_session(SessionRef::new("ssh:box", "work"), cx);
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
        assert!(!view.can_attach(&SessionRef::new("ssh:ghost", "work")));
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
        assert!(view.can_attach(&SessionRef::new("ssh:box", "work")));
        view.new_container(Some("Build"), cx).unwrap();
        view.attach_session(SessionRef::new("ssh:box", "work"), cx);
        let containers = view.control_containers();
        assert_eq!(containers[1].sessions.len(), 1);
        assert_eq!(view.containers.index_of(containers[1].id), Some(1));
        assert_eq!(view.containers.index_of(99), None);
        view.switch_container(0, cx);
        assert!(view.control_containers()[0].active);
    });
}
