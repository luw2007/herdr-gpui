use super::*;

#[gpui::test]
fn sessions_never_opens_without_its_target_window(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        for (caller, expected) in [
            (Caller::default(), ErrorCode::NoWindow),
            (
                Caller {
                    window: Some(42),
                    ..Caller::default()
                },
                ErrorCode::NotFound,
            ),
            (
                Caller {
                    workspace_id: Some("missing-workspace".into()),
                    daemon_socket: Some("/tmp/missing-session.sock".into()),
                    ..Caller::default()
                },
                ErrorCode::WorkspaceNotFound,
            ),
        ] {
            let response = command(
                Call {
                    caller,
                    op: CommandOp::Sessions {
                        query: "project feature/a".into(),
                    },
                },
                cx,
            );
            assert_eq!(code(&response), Some(expected));
        }
    });
}

#[gpui::test]
fn malformed_sessions_queries_are_refused_before_opening(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        for query in ["x".repeat(4097), "project\u{1b}[2J".into()] {
            let response = command(call(CommandOp::Sessions { query }), cx);
            assert_eq!(code(&response), Some(ErrorCode::InvalidRequest));
        }
    });
}
