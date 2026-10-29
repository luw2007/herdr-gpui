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
                        device: None,
                        project: None,
                        unassigned: false,
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
            let response = command(
                call(CommandOp::Sessions {
                    query,
                    device: None,
                    project: None,
                    unassigned: false,
                }),
                cx,
            );
            assert_eq!(code(&response), Some(ErrorCode::InvalidRequest));
        }
    });
}

#[gpui::test]
fn an_unknown_device_or_project_is_not_found_and_exits_5(cx: &mut gpui::TestAppContext) {
    let window = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    let caller = Caller {
        window: Some(window.window_id().as_u64()),
        ..Caller::default()
    };
    cx.update(|cx| {
        for (device, project) in [
            (Some("no-such-device"), None),
            (None, Some("no-such-project")),
        ] {
            let response = command(
                Call {
                    caller: caller.clone(),
                    op: CommandOp::Sessions {
                        query: String::new(),
                        device: device.map(str::to_owned),
                        project: project.map(str::to_owned),
                        unassigned: false,
                    },
                },
                cx,
            );
            assert_eq!(code(&response), Some(ErrorCode::NotFound), "{response:?}");
            let Response::Error { code, message } = response else {
                unreachable!()
            };
            let error = crate::Error::ControlRejected { code, message };
            assert_eq!(
                crate::control::client::status(&error),
                crate::control::client::exit::NOT_FOUND
            );
        }
    });
}

#[gpui::test]
fn sessions_without_a_device_starts_where_the_footer_is(cx: &mut gpui::TestAppContext) {
    let window = cx.add_window(|window, cx| {
        let mut view = crate::sidebar::layout_tests::fixture_window(window, cx);
        view.endpoints.push(crate::endpoint::Endpoint::new(
            "ssh:build".into(),
            "Build".into(),
            ConnectTarget::Ssh {
                target: "build.invalid".into(),
                session: "default".into(),
            },
            true,
        ));
        view.list_all_endpoints();
        view.device_filter = Some("ssh:build".into());
        view
    });
    let caller = Caller {
        window: Some(window.window_id().as_u64()),
        ..Caller::default()
    };
    for (device, expected) in [
        // Omitted: the manager opens on the footer's device.
        (None, Some("ssh:build")),
        // An explicit device wins over the footer.
        (Some("local"), Some("local")),
        (Some("Build"), Some("ssh:build")),
    ] {
        let response = cx.update(|cx| {
            command(
                Call {
                    caller: caller.clone(),
                    op: CommandOp::Sessions {
                        query: String::new(),
                        device: device.map(str::to_owned),
                        project: None,
                        unassigned: false,
                    },
                },
                cx,
            )
        });
        assert!(matches!(response, Response::Done { .. }), "{response:?}");
        window
            .update(cx, |view, _, _| {
                assert_eq!(
                    view.session_device_filter().as_deref(),
                    expected,
                    "{device:?}"
                );
                // The filter is the manager's copy; the footer keeps its own.
                assert_eq!(view.device_filter.as_deref(), Some("ssh:build"));
                assert_eq!(view.menu.page, Some(crate::menu::Page::Sessions));
            })
            .unwrap();
        window
            .update(cx, |view, window, cx| view.dismiss_menu(window, cx))
            .unwrap();
    }
}
