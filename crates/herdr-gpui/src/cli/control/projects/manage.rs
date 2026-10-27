#![allow(clippy::unwrap_used)]
use super::ProjectCommand;
use crate::cli::{LaunchMode, LaunchOptions, control::ControlCommand};

#[test]
fn manage_accepts_global_targeting_and_json_without_project_arguments() {
    for args in [
        vec!["project", "manage"],
        vec!["project", "manage", "--window", "7", "--json"],
        vec!["--json", "project", "manage", "--window", "7"],
        vec!["--window", "7", "--json", "project", "manage"],
        vec!["--json", "--window", "7", "project", "manage"],
    ] {
        let LaunchMode::Control(invocation) = LaunchOptions::parse(args.clone()).unwrap().mode
        else {
            panic!("not control: {args:?}");
        };
        assert_eq!(
            invocation.command,
            ControlCommand::Project(ProjectCommand::Manage)
        );
        assert_eq!(invocation.json, args.contains(&"--json"));
        assert_eq!(invocation.window, args.contains(&"--window").then_some(7));
    }
}

#[test]
fn manage_refuses_extra_project_arguments_and_invalid_targeting() {
    for args in [
        vec!["project", "manage", "name"],
        vec!["project", "manage", "--device", "local"],
        vec!["project", "manage", "--window", "invalid"],
        vec!["project", "manage", "--window", "7", "--window", "8"],
        vec!["--window", "invalid", "--json", "project", "manage"],
        vec![
            "--json", "project", "manage", "--window", "7", "--window", "8",
        ],
    ] {
        assert!(LaunchOptions::parse(args.clone()).is_err(), "{args:?}");
    }
}

#[test]
fn manage_help_describes_the_real_manager_workflow() {
    let LaunchMode::Control(invocation) = LaunchOptions::parse(["project", "manage", "--help"])
        .unwrap()
        .mode
    else {
        panic!("not help");
    };
    assert_eq!(invocation.command, ControlCommand::Help(Some("project")));
}

#[test]
fn leading_json_requests_json_for_control_usage_errors() {
    use std::ffi::OsString;
    let words: Vec<OsString> = ["--json", "--window", "7", "project", "manage", "extra"]
        .into_iter()
        .map(Into::into)
        .collect();
    assert!(crate::cli::control::wants_json(&words));
    assert!(LaunchOptions::parse(words).is_err());
    let not_control: Vec<OsString> = ["--window", "project", "--json"]
        .into_iter()
        .map(Into::into)
        .collect();
    assert!(!crate::cli::control::wants_json(&not_control));
}
