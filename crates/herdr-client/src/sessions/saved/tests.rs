use super::*;
use crate::sessions::{Error, RemoteSession, list_with, parse_session_list};
use std::{
    env,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const LINKED: &str = r#"{"version":3,"workspaces":[{"id":"w1","custom_name":null,
"identity_cwd":"/work/app-wt","worktree_space":{"key":"k","label":"app",
"repo_root":"/work/app","checkout_path":"/work/app-wt","is_linked_worktree":true},
"tabs":[]},{"id":"w2","custom_name":"notes","identity_cwd":"/work/notes","tabs":[]}]}"#;

fn root() -> PathBuf {
    let root = env::temp_dir().join(format!(
        "herdr-saved-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("sessions")).unwrap();
    root
}

use std::path::PathBuf;

fn workspace(project: &str, path: &str, label: Option<&str>) -> SessionWorkspace {
    SessionWorkspace {
        project: project.into(),
        path: path.into(),
        branch: None,
        label: label.map(str::to_owned),
        repo: None,
        ..Default::default()
    }
}

#[test]
fn a_linked_worktree_belongs_to_its_repository_and_several_workspaces_are_kept() {
    assert_eq!(
        parse_snapshot(LINKED.as_bytes()),
        [
            workspace("/work/app", "/work/app-wt", Some("app")),
            workspace("/work/notes", "/work/notes", Some("notes")),
        ]
    );
}

#[test]
fn unusable_snapshots_are_unclassified_not_errors() {
    for bytes in [
        &b""[..],
        b"not json",
        b"[]",
        br#"{"workspaces":"x"}"#,
        br#"{"workspaces":[{"identity_cwd":""},{"identity_cwd":"bad\u0007"},7]}"#,
    ] {
        assert!(parse_snapshot(bytes).is_empty(), "{bytes:?}");
    }
    let huge = vec![b' '; SNAPSHOT_MAX + 1];
    assert!(parse_snapshot(&huge).is_empty());
    let long = format!(
        r#"{{"workspaces":[{{"identity_cwd":"/{}"}}]}}"#,
        "a".repeat(TEXT_MAX)
    );
    assert!(parse_snapshot(long.as_bytes()).is_empty());
}

#[test]
fn workspaces_are_bounded_and_deduplicated() {
    let one = r#"{"identity_cwd":"/same"}"#;
    let same = format!(r#"{{"workspaces":[{one},{one}]}}"#);
    assert_eq!(parse_snapshot(same.as_bytes()).len(), 1);
    let many: Vec<String> = (0..WORKSPACES_MAX + 10)
        .map(|index| format!(r#"{{"identity_cwd":"/p{index}"}}"#))
        .collect();
    let many = format!(r#"{{"workspaces":[{}]}}"#, many.join(","));
    assert_eq!(parse_snapshot(many.as_bytes()).len(), WORKSPACES_MAX);
}

#[test]
fn a_stated_branch_is_kept_and_never_guessed() {
    let stated = br#"{"workspaces":[{"identity_cwd":"/p","branch":"feat/x"}]}"#;
    assert_eq!(parse_snapshot(stated)[0].branch.as_deref(), Some("feat/x"));
    assert_eq!(parse_snapshot(LINKED.as_bytes())[0].branch, None);
}

#[test]
fn local_sessions_carry_saved_workspaces_and_one_bad_file_does_not_fail_the_list() {
    let root = root();
    let sessions = root.join("sessions");
    for name in ["dx-wt-0123456789abcdef", "broken", "huge", "empty"] {
        fs::create_dir(sessions.join(name)).unwrap();
    }
    fs::write(sessions.join("dx-wt-0123456789abcdef/session.json"), LINKED).unwrap();
    fs::write(sessions.join("broken/session.json"), "{truncated").unwrap();
    fs::write(
        sessions.join("huge/session.json"),
        vec![b'{'; SNAPSHOT_MAX + 10],
    )
    .unwrap();
    fs::write(root.join("session.json"), LINKED).unwrap();
    let listed = list_with(&root, |_| false).unwrap();
    let find = |name: &str| listed.iter().find(|s| s.name == name).unwrap();
    assert_eq!(find("default").workspaces.len(), 2);
    assert_eq!(
        find("dx-wt-0123456789abcdef").workspaces[0].project,
        "/work/app"
    );
    for name in ["broken", "huge", "empty"] {
        assert!(find(name).workspaces.is_empty(), "{name}");
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_symlinked_session_file_is_never_read() {
    let root = root();
    let dir = root.join("sessions/linked");
    fs::create_dir(&dir).unwrap();
    fs::write(root.join("outside.json"), LINKED).unwrap();
    std::os::unix::fs::symlink(root.join("outside.json"), dir.join("session.json")).unwrap();
    assert!(read_snapshot(&dir).is_empty());
    fs::remove_dir_all(root).unwrap();
}

fn record(name: &str, bytes: &[u8]) -> String {
    format!("{BEGIN}{name}\n{}\n{END}\n", STANDARD.encode(bytes))
}

const JSON: &str = r#"{"sessions":[{"name":"default","running":true},{"name":"dx-wt-abc","running":false},{"name":"bad","running":false},{"name":"other","running":true}]}"#;

#[test]
fn remote_records_attach_by_name_and_damage_stays_with_its_own_session() {
    let output = format!(
        "banner\n{JSON}\n{}{}{BEGIN}bad\n!!!not base64!!!\n{END}\n{BEGIN}dangling\n{}\n{DONE}\n",
        record("dx-wt-abc", LINKED.as_bytes()),
        record(
            "default",
            br#"{"workspaces":[{"identity_cwd":"/home/me"}]}"#
        ),
        STANDARD.encode(LINKED)
    );
    let sessions = parse_session_list(output.as_bytes()).unwrap();
    let find = |name: &str| sessions.iter().find(|s| s.name == name).unwrap();
    assert_eq!(find("default").workspaces[0].project, "/home/me");
    assert_eq!(find("dx-wt-abc").workspaces.len(), 2);
    assert!(find("bad").workspaces.is_empty());
    assert!(find("other").workspaces.is_empty());
}

#[test]
fn records_for_unlisted_or_invalid_names_never_leak_into_other_sessions() {
    let output = format!(
        "{JSON}\n{}{}{DONE}\n",
        record("../escape", LINKED.as_bytes()),
        record("ghost", LINKED.as_bytes())
    );
    let sessions = parse_session_list(output.as_bytes()).unwrap();
    assert!(sessions.iter().all(|session| session.workspaces.is_empty()));
}

#[test]
fn an_oversized_record_is_dropped_alone() {
    let big = format!(
        "{JSON}\n{BEGIN}default\n{}\n{END}\n{}",
        "A".repeat(ENCODED_MAX + 1),
        record("other", LINKED.as_bytes())
    );
    let sessions = parse_session_list(big.as_bytes()).unwrap();
    assert!(sessions[0].workspaces.is_empty());
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.name == "other")
            .unwrap()
            .workspaces
            .len(),
        2
    );
}

#[test]
fn legacy_hosts_without_records_still_list() {
    let sessions = parse_session_list(JSON.as_bytes()).unwrap();
    assert_eq!(sessions.len(), 4);
    assert!(
        sessions
            .iter()
            .all(|s: &RemoteSession| s.workspaces.is_empty())
    );
    assert!(matches!(
        parse_session_list(format!("{BEGIN}default\n{END}\n").as_bytes()),
        Err(Error::SshClosed)
    ));
}
