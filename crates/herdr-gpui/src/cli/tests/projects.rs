use super::*;
use crate::cli::control::{ControlCommand, Invocation, ProjectCommand};

fn parsed(args: &[&str]) -> Result<Invocation, CliError> {
    match LaunchOptions::parse(args.iter().copied())?.mode {
        LaunchMode::Control(invocation) => Ok(invocation),
        other => panic!("not a control command: {other:?}"),
    }
}

fn command(args: &[&str]) -> ProjectCommand {
    match parsed(args).unwrap().command {
        ControlCommand::Project(command) => command,
        other => panic!("not a project command: {other:?}"),
    }
}

fn some(text: &str) -> Option<String> {
    Some(text.to_owned())
}

#[test]
fn project_commands_parse_with_their_options_anywhere() {
    assert_eq!(command(&["project"]), ProjectCommand::List);
    assert_eq!(command(&["project", "list"]), ProjectCommand::List);
    assert_eq!(
        command(&["project", "create", "Big", "Task"]),
        ProjectCommand::Create {
            name: "Big Task".into(),
            repo: None
        }
    );
    assert_eq!(
        command(&["project", "create", "--task", "Codename"]),
        ProjectCommand::Create {
            name: "Codename".into(),
            repo: None
        }
    );
    assert_eq!(
        command(&[
            "project",
            "create",
            "Repo",
            "--repo",
            "git@github.com:o/r.git"
        ]),
        ProjectCommand::Create {
            name: "Repo".into(),
            repo: some("git@github.com:o/r.git")
        }
    );
    assert_eq!(
        command(&["project", "rename", "old", "New", "name"]),
        ProjectCommand::Rename {
            project: "old".into(),
            name: "New name".into()
        }
    );
    assert_eq!(
        command(&["project", "bind", "alpha", "/work/a", "--device", "ssh:1"]),
        ProjectCommand::Bind {
            project: "alpha".into(),
            path: "/work/a".into(),
            device: some("ssh:1")
        }
    );
    assert_eq!(
        command(&["project", "unbind", "--device", "local", "/work/a"]),
        ProjectCommand::Unbind {
            path: "/work/a".into(),
            device: some("local")
        }
    );
    assert_eq!(
        command(&["project", "assign", "alpha"]),
        ProjectCommand::Assign {
            project: "alpha".into(),
            session: None,
            device: None
        }
    );
    assert_eq!(
        command(&[
            "project",
            "assign",
            "--session",
            "work",
            "alpha",
            "--device",
            "local"
        ]),
        ProjectCommand::Assign {
            project: "alpha".into(),
            session: some("work"),
            device: some("local")
        }
    );
    assert_eq!(
        command(&["project", "unassign", "--session", "work"]),
        ProjectCommand::Unassign {
            session: some("work"),
            device: None
        }
    );
    assert_eq!(
        command(&["project", "show"]),
        ProjectCommand::Show {
            session: None,
            device: None
        }
    );
    assert_eq!(
        command(&["project", "delete", "alpha"]),
        ProjectCommand::Delete {
            project: "alpha".into()
        }
    );
}

#[test]
fn every_project_command_takes_json_and_window() {
    for args in [
        &["project", "list"][..],
        &["project", "create", "A"],
        &["project", "rename", "a", "b"],
        &["project", "bind", "a", "/p"],
        &["project", "unbind", "/p"],
        &["project", "assign", "a"],
        &["project", "unassign"],
        &["project", "show"],
        &["project", "delete", "a"],
    ] {
        let mut with = args.to_vec();
        with.extend(["--json", "--window", "7"]);
        let invocation = parsed(&with).unwrap();
        assert!(invocation.json, "{args:?}");
        assert_eq!(invocation.window, Some(7), "{args:?}");
        let plain = parsed(args).unwrap();
        assert_eq!((plain.json, plain.window), (false, None), "{args:?}");
    }
}

#[test]
fn project_commands_reject_missing_extra_and_conflicting_arguments() {
    for args in [
        vec!["project", "frobnicate"],
        vec!["project", "create"],
        vec!["project", "create", "A", "--repo"],
        vec!["project", "create", "A", "--repo", "x", "--task"],
        vec!["project", "create", "A", "--repo", "x", "--repo", "y"],
        vec!["project", "rename", "a"],
        vec!["project", "bind", "a"],
        vec!["project", "bind", "a", "/p", "/q"],
        vec!["project", "unbind"],
        vec!["project", "assign"],
        vec!["project", "assign", "a", "b"],
        vec!["project", "show", "extra"],
        vec!["project", "show", "--session"],
        vec!["project", "show", "--session", "a", "--session", "b"],
        vec!["project", "delete"],
        vec!["project", "delete", "a", "b"],
        vec!["project", "list", "--bogus"],
        vec!["project", "list", "--window", "x"],
    ] {
        assert!(parsed(&args).is_err(), "{args:?}");
    }
}

#[test]
fn project_help_wins_and_errors_ask_for_json_when_it_does() {
    assert_eq!(
        parsed(&["project", "bind", "--help"]).unwrap().command,
        ControlCommand::Help(Some("project"))
    );
    let args: Vec<OsString> = ["project", "bogus", "--json"]
        .into_iter()
        .map(Into::into)
        .collect();
    assert!(control::wants_json(&args));
}

#[test]
fn project_paths_may_be_any_text_and_non_utf8_is_refused() {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let args = [
            OsString::from("project"),
            "bind".into(),
            "a".into(),
            OsString::from_vec(vec![b'/', 0xff]),
        ];
        assert!(LaunchOptions::parse(args).is_err());
    }
    assert_eq!(
        command(&["project", "bind", "a", "/work/项目"]),
        ProjectCommand::Bind {
            project: "a".into(),
            path: "/work/项目".into(),
            device: None
        }
    );
}
