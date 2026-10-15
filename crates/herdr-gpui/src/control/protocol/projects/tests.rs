#![allow(clippy::unwrap_used)]
use super::*;
use crate::control::protocol::{Call, Caller, Request, Response};

fn request(op: ProjectOp) -> Request {
    Request::Project(Call {
        caller: Caller::default(),
        op,
    })
}

#[test]
fn project_requests_have_a_stable_wire_form() {
    let text = serde_json::to_string(&request(ProjectOp::Bind {
        project: "alpha".into(),
        path: "/work/a".into(),
        device: None,
    }))
    .unwrap();
    assert_eq!(
        text,
        r#"{"method":"project","caller":{"pane_id":null,"workspace_id":null,"daemon_socket":null},"op":{"op":"bind","project":"alpha","path":"/work/a"}}"#
    );
    for op in [
        ProjectOp::List,
        ProjectOp::Create {
            name: "A".into(),
            repo: Some("github.com/o/r".into()),
        },
        ProjectOp::Create {
            name: "A".into(),
            repo: None,
        },
        ProjectOp::Rename {
            project: "a".into(),
            name: "b".into(),
        },
        ProjectOp::Unbind {
            path: "/x".into(),
            device: Some("ssh:1".into()),
        },
        ProjectOp::Assign {
            project: "a".into(),
            session: Some("s".into()),
            device: Some("local".into()),
        },
        ProjectOp::Unassign {
            session: None,
            device: None,
        },
        ProjectOp::Show {
            session: Some("s".into()),
            device: None,
        },
        ProjectOp::Delete {
            project: "a".into(),
        },
    ] {
        let wire = serde_json::to_string(&request(op.clone())).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&wire).unwrap(), request(op));
    }
}

#[test]
fn malformed_project_requests_are_rejected() {
    for invalid in [
        r#"{"method":"project","caller":{},"op":{"op":"destroy"}}"#,
        r#"{"method":"project","caller":{},"op":{"op":"create"}}"#,
        r#"{"method":"project","caller":{},"op":{"op":"bind","project":"a"}}"#,
        r#"{"method":"project","caller":{},"op":{"op":"delete","project":"a","force":true}}"#,
        r#"{"method":"project","caller":{},"op":{"op":"show","session":7}}"#,
        r#"{"method":"project","caller":{},"op":{"op":"list"},"extra":1}"#,
    ] {
        assert!(
            serde_json::from_str::<Request>(invalid).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn replies_round_trip() {
    let project = ProjectInfo {
        id: "i".into(),
        name: "n".into(),
        repo: None,
        bindings: vec![BindingInfo {
            device: "local".into(),
            path: "/p".into(),
        }],
        bindings_total: 1,
        sessions: 2,
    };
    for reply in [
        Reply::Projects {
            projects: vec![project.clone()],
            total: 9,
        },
        Reply::Project { project },
        Reply::Resolved(Box::new(Resolved {
            device: "local".into(),
            session: "s".into(),
            project: Some(ProjectRef {
                id: "i".into(),
                name: "n".into(),
                repo: Some("github.com/o/r".into()),
            }),
            source: Some(SourceInfo::Repository {
                repo: "github.com/o/r".into(),
            }),
            note: None,
        })),
        Reply::DeletePending {
            project: ProjectRef {
                id: "i".into(),
                name: "n".into(),
                repo: None,
            },
        },
    ] {
        let response = Response::Project { reply };
        let text = serde_json::to_string(&response).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&text).unwrap(), response);
    }
}

#[test]
fn values_are_bounded_and_printable() {
    let ok = |op: ProjectOp| op.validate().is_ok();
    assert!(ok(ProjectOp::List));
    assert!(ok(ProjectOp::Create {
        name: "x".repeat(MAX_PROJECT_REF),
        repo: Some("https://user:token@github.com/o/r.git".into()),
    }));
    assert!(!ok(ProjectOp::Create {
        name: "x".repeat(MAX_PROJECT_REF + 1),
        repo: None,
    }));
    assert!(!ok(ProjectOp::Create {
        name: String::new(),
        repo: None,
    }));
    for repo in [
        "/local/path",
        "not a repo",
        "file:///x/y",
        "github.com/only",
    ] {
        assert!(
            !ok(ProjectOp::Create {
                name: "n".into(),
                repo: Some(repo.into()),
            }),
            "{repo}"
        );
    }
    assert!(!ok(ProjectOp::Delete {
        project: "a\u{1b}[2J".into()
    }));
    assert!(!ok(ProjectOp::Bind {
        project: "a".into(),
        path: "x".repeat(MAX_PATH + 1),
        device: None,
    }));
    assert!(!ok(ProjectOp::Show {
        session: Some("../escape".into()),
        device: None,
    }));
    assert!(!ok(ProjectOp::Show {
        session: Some("s".repeat(MAX_SESSION + 1)),
        device: None,
    }));
    assert!(!ok(ProjectOp::Unassign {
        session: None,
        device: Some("d".repeat(MAX_DEVICE + 1)),
    }));
    assert!(ok(ProjectOp::Assign {
        project: "a".into(),
        session: Some("dx-wt-0a1b".into()),
        device: Some("wsl:Ubuntu".into()),
    }));
}

#[test]
fn a_repository_is_a_key_or_a_remote_url() {
    let key = parse_repo("github.com/Owner/Repo").unwrap();
    assert_eq!(key.as_str(), "github.com/owner/repo");
    assert_eq!(
        parse_repo("git@github.com:owner/repo.git"),
        Some(key.clone())
    );
    assert_eq!(parse_repo(" https://github.com/owner/repo/ "), Some(key));
    assert_eq!(parse_repo("owner/repo"), None);
}

#[test]
fn project_requests_never_extend_the_answer_timeout() {
    assert_eq!(request(ProjectOp::List).wait_seconds(), 0);
}
