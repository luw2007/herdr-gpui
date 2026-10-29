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
                    query: query.into(),
                    device: None,
                    project: None,
                    unassigned: false,
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
fn space_commands_parse_with_json_and_native_window_targeting() {
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
        mode(&["space"]).unwrap(),
        control(ContainerCommand::List, false, None)
    );
    assert_eq!(
        mode(&["space", "overview", "--json", "--window", "9"]).unwrap(),
        control(ContainerCommand::Overview, true, Some(9))
    );
    assert_eq!(
        mode(&["--json", "space", "overview"]).unwrap(),
        control(ContainerCommand::Overview, true, None)
    );
    assert_eq!(
        mode(&["space", "new", "--title", "Build", "--json"]).unwrap(),
        control(
            ContainerCommand::New {
                title: Some("Build".into())
            },
            true,
            None
        )
    );
    assert_eq!(
        mode(&["space", "focus", "Build", "--window", "9"]).unwrap(),
        control(
            ContainerCommand::Focus {
                window: "Build".into()
            },
            false,
            Some(9)
        )
    );
    assert_eq!(
        mode(&["space", "attach", "work", "--device", "ssh:box"]).unwrap(),
        control(
            ContainerCommand::Attach {
                session: "work".into(),
                device: Some("ssh:box".into()),
                window: None
            },
            false,
            None
        )
    );
    assert_eq!(
        mode(&["space", "detach", "work", "--in", "Build"]).unwrap(),
        control(
            ContainerCommand::Detach {
                session: "work".into(),
                device: None,
                window: Some("Build".into())
            },
            false,
            None
        )
    );
    assert_eq!(
        mode(&["space", "group", "Build", "mr"]).unwrap(),
        control(
            ContainerCommand::Group {
                window: "Build".into(),
                by: "mr".into()
            },
            false,
            None
        )
    );
    for bad in [
        vec!["space", "focus"],
        vec!["space", "attach"],
        vec!["space", "stop"],
        vec!["space", "new", "--bogus"],
    ] {
        assert!(mode(&bad).is_err(), "{bad:?}");
    }
    // The old external name is gone, not an alias.
    for old in [vec!["container"], vec!["container", "list"]] {
        assert_eq!(
            mode(&old).unwrap_err(),
            CliError::UnknownOption("container".into()),
            "{old:?}"
        );
    }
}

#[test]
fn sessions_device_and_project_filters_parse_and_conflict_is_refused() {
    let mode = |args: &[&str]| LaunchOptions::parse(args.iter().copied()).map(|o| o.mode);
    let sessions = |query: &str, device: Option<&str>, project: Option<&str>, unassigned| {
        LaunchMode::Control(Invocation {
            command: ControlCommand::Sessions {
                query: query.into(),
                device: device.map(Into::into),
                project: project.map(Into::into),
                unassigned,
            },
            json: false,
            window: None,
        })
    };
    assert_eq!(
        mode(&[
            "sessions",
            "--device",
            "ssh:box",
            "--project",
            "App",
            "feature"
        ])
        .unwrap(),
        sessions("feature", Some("ssh:box"), Some("App"), false)
    );
    assert_eq!(
        mode(&["sessions", "--no-project"]).unwrap(),
        sessions("", None, None, true)
    );
    assert!(mode(&["sessions", "--project", "App", "--no-project"]).is_err());
    assert!(mode(&["sessions", "--device"]).is_err());
    assert!(mode(&["sessions", "--device", "a", "--device", "b"]).is_err());
}
