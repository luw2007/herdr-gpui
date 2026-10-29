#![allow(clippy::unwrap_used)]
use super::*;
use crate::control::protocol::container::WindowRef;

#[test]
fn close_reports_that_the_confirmation_is_pending() {
    let reply = Reply::ClosePending {
        window: WindowRef {
            id: "7a8f3b0e".into(),
            title: "Window 1".into(),
        },
    };
    let output = output(&reply).unwrap();
    assert!(output.text.contains("Asked to close Window 1 (7a8f3b0e)"));
    assert!(output.text.contains("Nothing is closed yet"));
    assert_eq!(output.json["kind"], "close_pending");
    assert_eq!(output.json["window"]["id"], "7a8f3b0e");
    assert_eq!(output.status, 0);
}

#[test]
fn a_list_is_printed_with_its_selection_and_cut() {
    use crate::control::protocol::container::{ContainerInfo, SlotInfo};
    let reply = Reply::Containers {
        containers: vec![ContainerInfo {
            id: "a".into(),
            title: "One".into(),
            grouping: "none".into(),
            active: true,
            sessions: vec![SlotInfo {
                device: "local".into(),
                session: "work".into(),
                selected: true,
                status: "online".into(),
            }],
            sessions_total: 2,
        }],
    };
    let output = output(&reply).unwrap();
    assert_eq!(
        output.text,
        "*a One (none)\n    > work on local: online\n    ... 1 more not listed; the answer is cut to fit"
    );
    assert_eq!(output.json[0]["id"], "a");
}
