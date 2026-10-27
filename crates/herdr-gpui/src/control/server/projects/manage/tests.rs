#![allow(clippy::unwrap_used)]
use super::super::project;
use crate::control::protocol::{Call, Caller, ErrorCode, Response, projects::ProjectOp};
use crate::{HerdrWindow, sidebar::layout_tests::fixture_window};
use gpui::{TestAppContext, WindowHandle};

fn window(cx: &mut TestAppContext) -> WindowHandle<HerdrWindow> {
    cx.add_window(fixture_window)
}

fn ask(op: ProjectOp, cx: &mut TestAppContext) -> Response {
    ask_as(Caller::default(), op, cx)
}

fn ask_as(caller: Caller, op: ProjectOp, cx: &mut TestAppContext) -> Response {
    cx.update(|cx| project(Call { caller, op }, cx))
}

fn code(response: &Response) -> Option<ErrorCode> {
    match response {
        Response::Error { code, .. } => Some(*code),
        _ => None,
    }
}

#[gpui::test]
fn manage_opens_with_an_empty_registry_and_returns_a_bounded_ack(cx: &mut TestAppContext) {
    let handle = window(cx);
    let before = cx.update(|cx| {
        crate::projects::Projects::read(cx, |registry| {
            registry.projects().into_iter().cloned().collect::<Vec<_>>()
        })
    });
    assert!(before.is_empty());
    let response = ask(ProjectOp::Manage {}, cx);
    let Response::Done { message } = response else {
        panic!("manager did not open: {response:?}");
    };
    assert!(message.contains("Opened the Projects manager"));
    assert!(message.len() < 256);
    let wire = serde_json::to_vec(&Response::Done {
        message: message.clone(),
    })
    .unwrap();
    assert!(wire.len() < 512);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&wire).unwrap()["message"],
        message
    );
    handle
        .read_with(cx, |view, _| assert!(view.project_manager_showing()))
        .unwrap();
    let after = cx.update(|cx| {
        crate::projects::Projects::read(cx, |registry| {
            registry.projects().into_iter().cloned().collect::<Vec<_>>()
        })
    });
    assert_eq!(before, after);
    assert_eq!(
        code(&ask(ProjectOp::Manage {}, cx)),
        Some(ErrorCode::WindowBusy)
    );
}

#[gpui::test]
fn manage_honors_explicit_windows_and_never_falls_back(cx: &mut TestAppContext) {
    let first = window(cx);
    let second = window(cx);
    let caller = Caller {
        window: Some(second.window_id().as_u64()),
        ..Caller::default()
    };
    assert!(matches!(
        ask_as(caller, ProjectOp::Manage {}, cx),
        Response::Done { .. }
    ));
    first
        .read_with(cx, |view, _| assert!(!view.project_manager_showing()))
        .unwrap();
    second
        .read_with(cx, |view, _| assert!(view.project_manager_showing()))
        .unwrap();
    let absent = Caller {
        window: Some(u64::MAX),
        ..Caller::default()
    };
    assert_eq!(
        code(&ask_as(absent, ProjectOp::Manage {}, cx)),
        Some(ErrorCode::NotFound)
    );
}

#[gpui::test]
fn manage_refuses_an_unmatched_caller_workspace(cx: &mut TestAppContext) {
    let handle = window(cx);
    let caller = Caller {
        workspace_id: Some("missing-workspace".into()),
        ..Caller::default()
    };
    let response = cx.update(|cx| {
        project(
            Call {
                caller,
                op: ProjectOp::Manage {},
            },
            cx,
        )
    });
    assert_eq!(code(&response), Some(ErrorCode::WorkspaceNotFound));
    handle
        .read_with(cx, |view, _| assert!(!view.project_manager_showing()))
        .unwrap();
}

#[gpui::test]
fn manage_requires_an_existing_window(cx: &mut TestAppContext) {
    assert_eq!(
        code(&ask(ProjectOp::Manage {}, cx)),
        Some(ErrorCode::NoWindow)
    );
}
