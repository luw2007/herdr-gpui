//! Detached HEAD, failed probes and upstreams stay distinguishable.
use super::*;

fn head_of(path: &Path) -> (HeadState, Option<String>) {
    let mut list = vec![workspace(path)];
    let scope = format!("test:head-{}", path.display());
    enrich(&scope, &mut [&mut list], probe_local);
    (list[0].head, list[0].branch.clone())
}

#[test]
fn a_detached_head_is_not_a_failed_probe() {
    let (root, main, linked) = repo();
    // The saved file claimed a branch; a detached HEAD must clear it.
    assert_eq!(head_of(&main), (HeadState::Branch, Some("main".into())));
    run(&linked, &["checkout", "-q", "--detach"]);
    assert_eq!(head_of(&linked), (HeadState::Detached, None));
    // A path git cannot probe keeps the saved branch but is unavailable.
    let gone = root.join("gone");
    assert_eq!(
        head_of(&gone),
        (HeadState::Unavailable, Some("saved".into()))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_unfinished_branch_read_replaces_nothing_and_is_unavailable() {
    let mut list = vec![workspace(Path::new("/head-cache/p"))];
    let probe = |paths: &[String]| -> Found {
        paths
            .iter()
            .map(|path| (path.clone(), Some(GitInfo::default())))
            .collect()
    };
    enrich("test:head-unknown", &mut [&mut list], probe);
    assert_eq!(list[0].branch.as_deref(), Some("saved"));
    assert_eq!(list[0].head, HeadState::Unavailable);
}

#[test]
fn the_local_probe_names_the_upstream_head_not_the_local_branch() {
    let (root, main, linked) = repo();
    run(
        &main,
        &["remote", "add", "origin", "git@github.com:o/r.git"],
    );
    run(
        &main,
        &[
            "remote",
            "add",
            "fork",
            "https://user:secret@github.com/me/r.git",
        ],
    );
    run(&linked, &["config", "branch.feature/x.remote", "fork"]);
    run(
        &linked,
        &["config", "branch.feature/x.merge", "refs/heads/pushed-name"],
    );
    let path = linked.to_str().unwrap().to_owned();
    let found = probe_local(std::slice::from_ref(&path));
    let upstream = found[&path]
        .as_ref()
        .unwrap()
        .upstream
        .known()
        .cloned()
        .unwrap();
    assert_eq!(upstream.repo.as_str(), "github.com/me/r");
    assert_eq!(upstream.branch, "pushed-name");
    let main_path = main.to_str().unwrap().to_owned();
    let found = probe_local(std::slice::from_ref(&main_path));
    assert_eq!(
        found[&main_path].as_ref().unwrap().upstream,
        UpstreamState::Absent
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn the_remote_script_reports_detached_heads_and_upstreams() {
    let (root, main, linked) = repo();
    run(&main, &["remote", "add", "fork", "git@github.com:me/r.git"]);
    run(&main, &["config", "branch.main.remote", "fork"]);
    run(&main, &["config", "branch.main.merge", "refs/heads/trunk"]);
    run(&linked, &["checkout", "-q", "--detach"]);
    let paths: Vec<String> = [&main, &linked, &root.join("gone")]
        .iter()
        .map(|p| p.to_str().unwrap().to_owned())
        .collect();
    let output = Command::new("/bin/sh")
        .args(["-c", &remote_script(&paths)])
        .output()
        .unwrap();
    let found = parse_remote(&paths, &output.stdout);
    let main_info = found[&paths[0]].as_ref().unwrap();
    assert!(!main_info.detached);
    assert_eq!(main_info.upstream.known().unwrap().branch, "trunk");
    assert_eq!(
        main_info.upstream.known().unwrap().repo.as_str(),
        "github.com/me/r"
    );
    let linked_info = found[&paths[1]].as_ref().unwrap();
    assert!(linked_info.detached);
    assert_eq!(linked_info.branch, None);
    assert_eq!(found[&paths[2]], None);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn records_from_before_head_state_cannot_claim_detached() {
    let paths = vec!["/a".to_owned()];
    let output = format!(
        "{RECORD}1:{}:{}:{}\n{}\n",
        encoded("/a/.git"),
        encoded(""),
        encoded("git@github.com:o/r.git"),
        super::super::super::saved::DONE
    );
    let info = parse_remote(&paths, output.as_bytes())["/a"]
        .clone()
        .unwrap();
    assert!(!info.detached);
    assert_eq!(info.upstream, UpstreamState::Absent);
}

/// The upstream state git reports for `main` after configuring it.
#[cfg(unix)]
fn upstream_after(config: &[(&str, &str)]) -> (UpstreamState, UpstreamState) {
    let (root, main, _) = repo();
    run(
        &main,
        &["remote", "add", "origin", "git@github.com:o/r.git"],
    );
    for (key, value) in config {
        run(&main, &["config", key, value]);
    }
    let path = main.to_str().unwrap().to_owned();
    let local = probe_local(std::slice::from_ref(&path))[&path]
        .clone()
        .unwrap()
        .upstream;
    let output = Command::new("/bin/sh")
        .args(["-c", &remote_script(std::slice::from_ref(&path))])
        .output()
        .unwrap();
    let remote = parse_remote(std::slice::from_ref(&path), &output.stdout)[&path]
        .clone()
        .unwrap()
        .upstream;
    fs::remove_dir_all(root).unwrap();
    (local, remote)
}

#[cfg(unix)]
#[test]
fn an_upstream_that_cannot_be_followed_is_unsupported_not_absent() {
    let heads = ("branch.main.merge", "refs/heads/main");
    let cases: [(&[(&str, &str)], UpstreamState); 8] = [
        // Nothing configured, or only half of it: genuinely no upstream.
        (&[], UpstreamState::Absent),
        (&[("branch.main.remote", "origin")], UpstreamState::Absent),
        // A local `.` remote, a ref outside refs/heads, a remote with no URL
        // and one whose URL names no repository are configured but unusable.
        (
            &[("branch.main.remote", "."), heads],
            UpstreamState::Unsupported,
        ),
        (
            &[
                ("branch.main.remote", "origin"),
                ("branch.main.merge", "refs/tags/v1"),
            ],
            UpstreamState::Unsupported,
        ),
        (
            &[("branch.main.remote", "ghost"), heads],
            UpstreamState::Unsupported,
        ),
        // Set but empty is configured, not absent: never a guess from origin.
        (
            &[("branch.main.remote", "origin"), ("branch.main.merge", "")],
            UpstreamState::Unsupported,
        ),
        (
            &[("branch.main.remote", ""), heads],
            UpstreamState::Unsupported,
        ),
        (
            &[("branch.main.remote", ""), ("branch.main.merge", "")],
            UpstreamState::Unsupported,
        ),
    ];
    for (config, expected) in cases {
        let (local, remote) = upstream_after(config);
        assert_eq!(local, expected, "{config:?}");
        assert_eq!(remote, expected, "{config:?} (remote script)");
    }
    let (local, remote) = upstream_after(&[
        ("remote.pathy.url", "/srv/git/local.git"),
        ("branch.main.remote", "pathy"),
        heads,
    ]);
    assert_eq!(
        (local, remote),
        (UpstreamState::Unsupported, UpstreamState::Unsupported)
    );
    let (local, remote) = upstream_after(&[("branch.main.remote", "origin"), heads]);
    for state in [local, remote] {
        assert_eq!(state.known().unwrap().repo.as_str(), "github.com/o/r");
    }
}

#[test]
fn a_remote_record_flags_an_unreadable_config_and_old_records_stay_readable() {
    let paths = vec!["/a".to_owned(), "/b".to_owned(), "/c".to_owned()];
    let common = encoded("/a/.git");
    let main = encoded("main");
    let url = encoded("git@github.com:o/r.git");
    let merge = encoded("refs/heads/main");
    let none = encoded("");
    let records = [
        format!("{RECORD}1:{common}:{main}:{none}:0:{none}:{none}:u"),
        format!("{RECORD}2:{common}:{main}:{none}:0:{url}:{merge}"),
        format!("{RECORD}3:{common}:{main}:{none}:0:{none}:{merge}:c"),
    ];
    let output = format!(
        "{}\n{}\n",
        records.join("\n"),
        super::super::super::saved::DONE
    );
    let found = parse_remote(&paths, output.as_bytes());
    assert_eq!(
        found["/a"].as_ref().unwrap().upstream,
        UpstreamState::Unavailable
    );
    // No state flag: the record predates it and keeps the earlier reading.
    assert!(found["/b"].as_ref().unwrap().upstream.known().is_some());
    assert_eq!(
        found["/c"].as_ref().unwrap().upstream,
        UpstreamState::Unsupported
    );
}

#[test]
fn a_path_git_cannot_probe_has_no_confirmed_upstream() {
    let mut list = vec![workspace(Path::new("/head-cache/q"))];
    enrich("test:upstream-gone", &mut [&mut list], |paths| {
        paths.iter().map(|p| (p.clone(), None)).collect()
    });
    assert_eq!(list[0].upstream, UpstreamState::Unavailable);
}
