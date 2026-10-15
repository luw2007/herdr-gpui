#![allow(clippy::unwrap_used)]
use super::*;
use crate::control::protocol::projects::{
    BindingInfo, ProjectInfo, ProjectRef, Resolved, SourceInfo,
};

fn info(name: &str, repo: Option<&str>, bindings: usize, sessions: usize) -> ProjectInfo {
    ProjectInfo {
        id: format!("id-{name}"),
        name: name.into(),
        repo: repo.map(Into::into),
        bindings: (0..bindings.min(2))
            .map(|n| BindingInfo {
                device: "local".into(),
                path: format!("/work/{name}/{n}"),
            })
            .collect(),
        bindings_total: bindings,
        sessions,
    }
}

#[test]
fn a_list_shows_kind_bindings_and_what_was_left_out() {
    let projects = [
        info("alpha", Some("github.com/o/alpha"), 1, 1),
        info("beta", None, 3, 0),
    ];
    let text = text::list(&projects, 5);
    assert_eq!(
        text,
        "id-alpha  alpha  [repo github.com/o/alpha]  1 assigned session\n    local: /work/alpha/0\n\
id-beta  beta  [task project]  0 assigned sessions\n    local: /work/beta/0\n    local: /work/beta/1\n    ... and 1 more\n\
... and 3 more projects"
    );
    assert!(text::list(&[], 0).starts_with("No projects yet"));
}

#[test]
fn an_empty_list_exits_nothing_and_json_keeps_the_reply() {
    let empty = Reply::Projects {
        projects: Vec::new(),
        total: 0,
    };
    let output = output(&empty).unwrap();
    assert_eq!(output.status, super::super::exit::NOTHING);
    assert_eq!(output.json["total"], 0);
    assert_eq!(output.json["projects"], serde_json::json!([]));
    let full = output_of(vec![info("a", None, 0, 0)]);
    assert_eq!(full.status, 0);
    assert_eq!(full.json["projects"][0]["name"], "a");
    assert_eq!(full.json["projects"][0]["repo"], serde_json::Value::Null);
}

fn output_of(projects: Vec<ProjectInfo>) -> Output {
    let total = projects.len();
    output(&Reply::Projects { projects, total }).unwrap()
}

#[test]
fn show_says_why_a_session_belongs_or_does_not() {
    let project = Some(ProjectRef {
        id: "p1".into(),
        name: "Alpha".into(),
        repo: None,
    });
    let base = Resolved {
        device: "local".into(),
        session: "work".into(),
        project: project.clone(),
        source: Some(SourceInfo::Assigned),
        note: None,
    };
    assert_eq!(
        text::resolved(&base),
        "Session work on local: Alpha (p1) via assigned"
    );
    let by_path = Resolved {
        source: Some(SourceInfo::Path {
            path: "/work/a".into(),
        }),
        ..base.clone()
    };
    assert!(text::resolved(&by_path).ends_with("via folder binding /work/a"));
    let by_repo = Resolved {
        source: Some(SourceInfo::Repository {
            repo: "github.com/o/r".into(),
        }),
        ..base.clone()
    };
    assert!(text::resolved(&by_repo).ends_with("via repository github.com/o/r"));
    let none = Resolved {
        project: None,
        source: None,
        note: Some("Not scanned".into()),
        ..base
    };
    assert_eq!(
        text::resolved(&none),
        "Session work on local: no project. Not scanned"
    );
    let output = output(&Reply::Resolved(Box::new(none))).unwrap();
    assert_eq!(output.json["project"], serde_json::Value::Null);
    assert_eq!(output.json["note"], "Not scanned");
}

#[test]
fn delete_reports_that_confirmation_is_pending() {
    let project = ProjectRef {
        id: "p1".into(),
        name: "Alpha".into(),
        repo: None,
    };
    let output = output(&Reply::DeletePending { project }).unwrap();
    assert!(output.text.contains("confirm in the Sessions manager"));
    assert!(output.text.contains("Nothing is deleted yet"));
    assert_eq!(output.json["kind"], "delete_pending");
    assert_eq!(output.status, 0);
}

#[test]
fn a_created_project_names_its_kind() {
    assert_eq!(
        text::created(&info("alpha", None, 0, 0)),
        "Created alpha (id-alpha) [task project]"
    );
}
