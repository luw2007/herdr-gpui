use super::*;
use crate::cli::control::{ControlCommand, Invocation};

#[test]
fn sessions_opens_the_manager_with_query_and_explicit_target() {
    for (args, query, json, window) in [
        (vec!["sessions"], "", false, None),
        (
            vec![
                "sessions",
                "my project",
                "feature/a",
                "--json",
                "--window",
                "42",
            ],
            "my project feature/a",
            true,
            Some(42),
        ),
        (vec!["sessions", "/work/项目"], "/work/项目", false, None),
    ] {
        assert_eq!(
            LaunchOptions::parse(args).unwrap().mode,
            LaunchMode::Control(Invocation {
                command: ControlCommand::Sessions {
                    query: query.into()
                },
                json,
                window,
            })
        );
    }
}

#[test]
fn sessions_rejects_unknown_flags_invalid_targets_and_unbounded_queries() {
    for args in [
        vec!["sessions", "project", "--stop"],
        vec!["sessions", "--window", "not-a-window"],
        vec!["sessions", "--window", "1", "--window", "2"],
        vec!["sessions", "bad\nquery"],
    ] {
        assert!(LaunchOptions::parse(args).is_err());
    }
    let at_limit = "q".repeat(4096);
    assert!(LaunchOptions::parse(["sessions", at_limit.as_str()]).is_ok());
    let oversized = "é".repeat(2049);
    assert!(LaunchOptions::parse(["sessions", oversized.as_str()]).is_err());
}

#[test]
fn container_commands_parse_with_json_and_native_window_targeting() {
    use crate::cli::control::ContainerCommand;
    let mode = |args: &[&str]| LaunchOptions::parse(args.iter().copied()).map(|o| o.mode);
    let control = |command, json, window| {
        LaunchMode::Control(Invocation {
            command: ControlCommand::Container(command),
            json,
            window,
        })
    };
    assert_eq!(
        mode(&["container"]).unwrap(),
        control(ContainerCommand::List, false, None)
    );
    assert_eq!(
        mode(&["container", "new", "--title", "Build", "--json"]).unwrap(),
        control(
            ContainerCommand::New {
                title: Some("Build".into())
            },
            true,
            None
        )
    );
    assert_eq!(
        mode(&["container", "focus", "3", "--window", "9"]).unwrap(),
        control(ContainerCommand::Focus { id: 3 }, false, Some(9))
    );
    assert_eq!(
        mode(&["container", "attach", "work", "--device", "ssh:box"]).unwrap(),
        control(
            ContainerCommand::Attach {
                session: "work".into(),
                device: Some("ssh:box".into())
            },
            false,
            None
        )
    );
    for bad in [
        vec!["container", "focus", "x"],
        vec!["container", "attach"],
        vec!["container", "stop"],
        vec!["container", "new", "--bogus"],
    ] {
        assert!(mode(&bad).is_err(), "{bad:?}");
    }
}
