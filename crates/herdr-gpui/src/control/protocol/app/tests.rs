#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn sessions_queries_are_bounded_and_wire_fields_are_strict() {
    assert!(valid_session_query(""));
    assert!(valid_session_query("project /work/a feature/a"));
    assert!(valid_session_query(&"q".repeat(MAX_SESSION_QUERY)));
    assert!(!valid_session_query(&"é".repeat(MAX_SESSION_QUERY / 2 + 1)));
    assert!(!valid_session_query("project\nbranch"));
    let op = CommandOp::Sessions {
        query: "/work/a feature/a".into(),
        device: None,
        project: None,
        unassigned: false,
    };
    let wire = serde_json::json!({"op": "sessions", "query": "/work/a feature/a"});
    assert_eq!(serde_json::to_value(&op).unwrap(), wire);
    assert_eq!(serde_json::from_value::<CommandOp>(wire).unwrap(), op);
    assert!(
        serde_json::from_value::<CommandOp>(serde_json::json!({
            "op": "sessions", "query": "a", "stop": true
        }))
        .is_err()
    );
}

#[test]
fn sessions_filters_travel_on_the_wire_only_when_set() {
    let op = CommandOp::Sessions {
        query: String::new(),
        device: Some("ssh:box".into()),
        project: None,
        unassigned: true,
    };
    let wire = serde_json::json!({
        "op": "sessions", "query": "", "device": "ssh:box", "unassigned": true
    });
    assert_eq!(serde_json::to_value(&op).unwrap(), wire);
    assert_eq!(serde_json::from_value::<CommandOp>(wire).unwrap(), op);
}
