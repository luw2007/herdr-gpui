use super::*;
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(status.success(), "{args:?}");
}

/// A repository with one linked worktree, under a canonical temporary directory.
fn repo() -> (PathBuf, PathBuf, PathBuf) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "herdr-git-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    let main = root.join("app");
    let linked = root.join("app-feature");
    fs::create_dir_all(&main).unwrap();
    run(&main, &["init", "-q"]);
    run(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
    run(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature/x",
            linked.to_str().unwrap(),
        ],
    );
    (root, main, linked)
}

fn workspace(path: &Path) -> SessionWorkspace {
    let path = path.to_str().unwrap().to_owned();
    SessionWorkspace {
        project: path.clone(),
        path,
        branch: Some("saved".into()),
        label: None,
    }
}

#[test]
fn project_of_resolves_relative_and_absolute_common_dirs_on_posix_rules() {
    let of = |path, common| project_of(path, common, true);
    assert_eq!(of("/w/app", ".git").as_deref(), Some("/w/app"));
    assert_eq!(of("/w/app/sub", "../.git").as_deref(), Some("/w/app"));
    assert_eq!(of("/w/wt", "/w/app/.git").as_deref(), Some("/w/app"));
    assert_eq!(of("/w/bare", ".").as_deref(), Some("/w/bare"));
    assert_eq!(of("/w//x/./y/../app", ".git").as_deref(), Some("/w/x/app"));
    assert_eq!(of("/repo", ".git").as_deref(), Some("/repo"));
    assert_eq!(of("/", ".git").as_deref(), Some("/"));
    assert_eq!(of("/w/x", ""), None);
    // Host-independent: a remote path never takes a Windows meaning.
    assert!(is_absolute("wsl:Ubuntu", "/home/me/app"));
    assert!(is_absolute("ssh:host", "/home/me/app"));
    assert!(!is_absolute("ssh:host", "relative/app"));
    assert!(!is_absolute("ssh:host", r"C:\app"));
    assert_eq!(posix_normalize("/a/../../b/./c//d"), "/b/c/d");
}

#[test]
fn every_worktree_of_a_repository_joins_one_project_with_its_live_branch() {
    let (root, main, linked) = repo();
    let mut one = vec![workspace(&main)];
    let mut two = vec![workspace(&linked)];
    enrich("test:worktrees", &mut [&mut one, &mut two], probe_local);
    let project = main.to_str().unwrap();
    assert_eq!(one[0].project, project);
    assert_eq!(two[0].project, project);
    assert_eq!(two[0].path, linked.to_str().unwrap());
    assert_eq!(two[0].branch.as_deref(), Some("feature/x"));
    assert_eq!(one[0].branch.as_deref(), Some("main"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_missing_or_non_repository_path_keeps_its_saved_values() {
    let (root, _, _) = repo();
    let plain = root.join("plain");
    fs::create_dir(&plain).unwrap();
    let mut list = vec![workspace(&plain), workspace(&root.join("gone"))];
    let before = list.clone();
    enrich("test:fallback", &mut [&mut list], probe_local);
    assert_eq!(list, before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_saved_worktree_group_is_not_replaced_and_relative_paths_are_never_probed() {
    let mut grouped = vec![SessionWorkspace {
        project: "/saved/repo".into(),
        path: "/saved/repo-wt".into(),
        branch: None,
        label: None,
    }];
    let mut relative = vec![workspace(Path::new("relative/path"))];
    let seen = std::cell::RefCell::new(Vec::new());
    enrich(
        "test:grouped",
        &mut [&mut grouped, &mut relative],
        |paths| {
            seen.borrow_mut().extend_from_slice(paths);
            paths
                .iter()
                .map(|path| {
                    let info = GitInfo {
                        project: Some("/other".into()),
                        branch: Some("live".into()),
                    };
                    (path.clone(), Some(info))
                })
                .collect()
        },
    );
    assert_eq!(grouped[0].project, "/saved/repo");
    assert_eq!(grouped[0].branch.as_deref(), Some("live"));
    assert_eq!(*seen.borrow(), ["/saved/repo-wt"]);
    assert_eq!(relative[0].branch.as_deref(), Some("saved"));
}

#[test]
fn a_failed_probe_overwrites_nothing_and_a_batch_is_resumed_by_the_next_refresh() {
    let paths: Vec<String> = (0..BATCH + 10)
        .map(|index| format!("/batch/p{index}"))
        .collect();
    let mut list: Vec<SessionWorkspace> = paths
        .iter()
        .map(|path| workspace(Path::new(path)))
        .collect();
    let info = |path: &String| {
        let info = GitInfo {
            project: Some("/repo".into()),
            branch: Some("b".into()),
        };
        (path.clone(), Some(info))
    };
    let first = std::cell::RefCell::new(0);
    enrich("test:batch", &mut [&mut list], |todo| {
        *first.borrow_mut() = todo.len();
        todo.iter().map(info).collect()
    });
    assert_eq!(*first.borrow(), BATCH);
    let second = std::cell::RefCell::new(Vec::new());
    enrich("test:batch", &mut [&mut list], |todo| {
        *second.borrow_mut() = todo.to_vec();
        Found::new()
    });
    assert_eq!(*second.borrow(), paths[BATCH..]);
    assert_eq!(list[0].project, "/repo");
    assert_eq!(list[BATCH].project, paths[BATCH]);
    assert_eq!(list[BATCH].branch.as_deref(), Some("saved"));
}

#[test]
fn the_cache_is_per_host_scope() {
    let mut a = vec![workspace(Path::new("/scoped/p"))];
    enrich("test:scope-a", &mut [&mut a], |paths| {
        paths
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    Some(GitInfo {
                        project: Some("/a".into()),
                        branch: None,
                    }),
                )
            })
            .collect()
    });
    let mut b = vec![workspace(Path::new("/scoped/p"))];
    let probed = std::cell::Cell::new(false);
    enrich("test:scope-b", &mut [&mut b], |_| {
        probed.set(true);
        Found::new()
    });
    assert_eq!(a[0].project, "/a");
    assert_eq!(b[0].project, "/scoped/p");
    assert!(probed.get());
}

#[cfg(unix)]
#[test]
fn the_remote_probe_runs_real_git_through_sh_with_quoted_paths() {
    let (root, main, linked) = repo();
    let odd = root.join("it's a \"dir\" $(touch pwned)");
    fs::create_dir(&odd).unwrap();
    let paths: Vec<String> = [&main, &linked, &odd, &root.join("gone")]
        .iter()
        .map(|p| p.to_str().unwrap().to_owned())
        .collect();
    let output = Command::new("/bin/sh")
        .args(["-c", &remote_script(&paths)])
        .output()
        .unwrap();
    let found = parse_remote(&paths, &output.stdout);
    assert!(!root.join("pwned").exists());
    assert_eq!(
        found[&paths[0]].as_ref().unwrap().branch.as_deref(),
        Some("main")
    );
    let linked_info = found[&paths[1]].as_ref().unwrap();
    assert_eq!(linked_info.project.as_deref(), main.to_str());
    assert_eq!(linked_info.branch.as_deref(), Some("feature/x"));
    assert_eq!(found[&paths[2]], None);
    assert_eq!(found[&paths[3]], None);
    // An unfinished script answers nothing, so saved values would be kept.
    let cut = String::from_utf8_lossy(&output.stdout).replace(super::super::saved::DONE, "");
    assert!(parse_remote(&paths, cut.as_bytes()).is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_remote_that_cannot_run_the_probe_changes_nothing() {
    let mut list = vec![workspace(Path::new("/remote/p"))];
    let before = list.clone();
    enrich_remote("test:remote-down", &mut [&mut list], |_| None);
    assert_eq!(list, before);
}
