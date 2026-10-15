//! Live git context for saved workspaces. Herdr's `session.json` only records a
//! worktree group when a worktree was made through its own API, so a session
//! started in a plain `git worktree` checkout would otherwise be its own
//! "project". A read-only `git` probe names the real repository and the checked
//! out branch instead; nothing is guessed from a session or directory name.
//!
//! Probes are bounded (unique paths per refresh, per-command and total
//! deadlines, output limit) and cached by path. A path that is missing, is not
//! a repository, or cannot be probed keeps its saved values, and a probe that
//! fails never fails a listing. Paths beyond one batch are probed by later
//! refreshes, which skip what is already cached.
use super::saved::SessionWorkspace;
use crate::{RepoKey, shell_quote, ssh};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{LazyLock, Mutex, PoisonError},
    time::{Duration, Instant},
};

/// Paths held in the cache at once.
const CACHE_MAX: usize = 4096;
/// How long a probe answer is trusted before it is asked again.
const TTL: Duration = Duration::from_secs(30);
/// Unique paths probed by one refresh.
const BATCH: usize = 256;
/// One local git command, and all of one local refresh's commands.
const STEP: Duration = Duration::from_secs(2);
const LOCAL_DEADLINE: Duration = Duration::from_secs(5);
/// The largest remote probe script.
const SCRIPT_MAX: usize = 48 * 1024;

/// Base64 of the longest remote URL a record may carry.
const URL_ENCODED_MAX: usize = 2048;

const RECORD: &str = "herdr-git:";

/// What git said about one checkout path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitInfo {
    pub project: Option<String>,
    pub branch: Option<String>,
    /// The normalized `origin` (else `upstream`) remote. The raw URL can embed
    /// credentials, so it is normalized where it is read and never kept.
    pub repo: Option<RepoKey>,
}

type Key = (String, String);
type Cache = HashMap<Key, (Instant, Option<GitInfo>)>;
type Found = HashMap<String, Option<GitInfo>>;

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

/// Whether `scope` names a host whose paths are POSIX regardless of the machine
/// this client runs on: SSH and WSL targets are Unix, only `local:` follows the
/// host's own path rules.
fn posix(scope: &str) -> bool {
    !scope.starts_with("local:")
}

fn is_absolute(scope: &str, path: &str) -> bool {
    if posix(scope) {
        path.starts_with('/')
    } else {
        Path::new(path).is_absolute()
    }
}

/// `path` with `.` and empty segments dropped and `..` applied, on POSIX rules.
/// `path` is absolute.
pub(crate) fn posix_normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The repository a checkout belongs to from `git rev-parse --git-common-dir`,
/// which is relative to `path` on older Git and for a main checkout: the
/// directory holding `.git`, or the common directory itself for a bare repository.
/// `posix` selects POSIX path rules (a remote or WSL host) over the host's own.
pub(crate) fn project_of(path: &str, common: &str, posix: bool) -> Option<String> {
    let common = common.trim();
    if common.is_empty() {
        return None;
    }
    let project = if posix {
        let joined = if common.starts_with('/') {
            common.to_owned()
        } else {
            format!("{path}/{common}")
        };
        let normal = posix_normalize(&joined);
        match normal.strip_suffix("/.git") {
            Some("") => "/".to_owned(),
            Some(parent) => parent.to_owned(),
            None => normal,
        }
    } else {
        let common = Path::new(common);
        let absolute = if common.is_absolute() {
            common.to_owned()
        } else {
            Path::new(path).join(common)
        };
        let absolute = lexical(&absolute);
        let project = if absolute.file_name().is_some_and(|name| name == ".git") {
            absolute.parent()?
        } else {
            &absolute
        };
        project.to_str()?.to_owned()
    };
    (project.len() <= 1024 && !project.chars().any(char::is_control)).then_some(project)
}

fn clean(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && text.len() <= 1024 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}

fn git(path: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["-C", path])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    let (status, output) = ssh::run(&mut command, timeout, || false).ok()?;
    status
        .success()
        .then(|| String::from_utf8(output).ok())
        .flatten()
}

/// The repository a remote URL names, or `None` for a missing, local, or
/// unusable one. The URL is dropped here.
fn repo_of(url: &str) -> Option<RepoKey> {
    RepoKey::from_remote_url(url)
}

/// `origin`'s URL, else `upstream`'s, as a normalized key.
fn remote_repo(path: &str, timeout: impl Fn() -> Duration) -> Option<RepoKey> {
    ["remote.origin.url", "remote.upstream.url"]
        .iter()
        .find_map(|key| repo_of(&git(path, &["config", "--get", key], timeout())?))
}

/// Probe local checkouts one after another until the batch or the deadline ends.
pub(crate) fn probe_local(paths: &[String]) -> Found {
    let deadline = Instant::now() + LOCAL_DEADLINE;
    let mut found = Found::new();
    // Each command gets what is left of the whole budget, at most `STEP`.
    let left = || deadline.saturating_duration_since(Instant::now()).min(STEP);
    for path in paths {
        if left().is_zero() {
            break;
        }
        let info = git(path, &["rev-parse", "--git-common-dir"], left()).map(|common| GitInfo {
            project: project_of(path, &common, false),
            branch: git(path, &["symbolic-ref", "--short", "-q", "HEAD"], left())
                .and_then(|branch| clean(&branch)),
            repo: remote_repo(path, left),
        });
        found.insert(path.clone(), info);
    }
    found
}

/// The POSIX script that probes `paths` on a remote host. Each path is one
/// quoted word and results are keyed by position, so nothing a path contains is
/// ever echoed back or interpreted.
pub(crate) fn remote_script(paths: &[String]) -> String {
    let words: Vec<String> = paths.iter().map(|path| shell_quote(path)).collect();
    format!(
        r#"export GIT_OPTIONAL_LOCKS=0 GIT_TERMINAL_PROMPT=0
i=0
for p in {}; do
    i=$((i + 1))
    common=$(git -C "$p" rev-parse --git-common-dir 2>/dev/null) || continue
    branch=$(git -C "$p" symbolic-ref --short -q HEAD 2>/dev/null || :)
    url=$(git -C "$p" config --get remote.origin.url 2>/dev/null || git -C "$p" config --get remote.upstream.url 2>/dev/null || :)
    c=$(printf %s "$common" | base64 | tr -d '\n\r')
    b=$(printf %s "$branch" | base64 | tr -d '\n\r')
    u=$(printf %s "$url" | base64 | tr -d '\n\r')
    printf '{RECORD}%s:%s:%s:%s\n' "$i" "$c" "$b" "$u"
done
printf '{}\n'"#,
        words.join(" "),
        super::saved::DONE
    )
}

/// `paths` that fit one script, in order.
fn fit(paths: &[String]) -> &[String] {
    let mut size = 512;
    let mut count = 0;
    for path in paths {
        size += path.len() * 4 + 8;
        if size > SCRIPT_MAX {
            break;
        }
        count += 1;
    }
    &paths[..count]
}

fn decode(text: &str) -> Option<String> {
    String::from_utf8(STANDARD.decode(text).ok()?).ok()
}

/// Records a finished remote probe printed. A path with no record was missing
/// or not a repository, which is an answer; an unfinished output answers nothing.
pub(crate) fn parse_remote(paths: &[String], output: &[u8]) -> Found {
    let mut found = Found::new();
    let mut done = false;
    for line in String::from_utf8_lossy(output).lines() {
        let line = line.trim_end();
        if line == super::saved::DONE {
            done = true;
        } else if let Some(record) = line.strip_prefix(RECORD) {
            let mut fields = record.split(':');
            let (Some(index), Some(common), Some(branch)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Some(path) = index
                .parse::<usize>()
                .ok()
                .and_then(|index| paths.get(index.checked_sub(1)?))
            else {
                continue;
            };
            // Records from before remotes were probed have no fourth field.
            let repo = fields
                .next()
                .filter(|url| url.len() <= URL_ENCODED_MAX)
                .and_then(decode)
                .and_then(|url| repo_of(&url));
            let info = GitInfo {
                project: decode(common).and_then(|common| project_of(path, &common, true)),
                branch: decode(branch).and_then(|branch| clean(&branch)),
                repo,
            };
            found.insert(path.clone(), Some(info));
        }
    }
    if done {
        for path in paths {
            found.entry(path.clone()).or_insert(None);
        }
    } else {
        found.clear();
    }
    found
}

/// Fill workspaces from the cache, probing one batch of what is uncached (then
/// what is stale) first. A workspace not tied to a saved worktree group takes
/// git's repository as its project; git's branch always replaces the saved one,
/// and a failed probe replaces nothing.
pub(crate) fn enrich(
    scope: &str,
    lists: &mut [&mut Vec<SessionWorkspace>],
    probe: impl FnOnce(&[String]) -> Found,
) {
    let mut seen = HashSet::new();
    let mut missing = Vec::new();
    let mut stale = Vec::new();
    {
        let cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        for workspace in lists.iter().flat_map(|list| list.iter()) {
            let path = &workspace.path;
            if !is_absolute(scope, path) || !seen.insert(path.clone()) {
                continue;
            }
            match cache.get(&(scope.to_owned(), path.clone())) {
                None => missing.push(path.clone()),
                Some((at, _)) if at.elapsed() >= TTL => stale.push(path.clone()),
                Some(_) => {}
            }
        }
    }
    let todo: Vec<String> = missing.into_iter().chain(stale).take(BATCH).collect();
    if !todo.is_empty() {
        let found = probe(&todo);
        let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() + found.len() > CACHE_MAX {
            cache.retain(|_, (at, _)| at.elapsed() < TTL);
        }
        if cache.len() + found.len() > CACHE_MAX {
            cache.clear();
        }
        let now = Instant::now();
        for (path, info) in found {
            cache.insert((scope.to_owned(), path), (now, info));
        }
    }
    let cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    for workspace in lists.iter_mut().flat_map(|list| list.iter_mut()) {
        let Some((_, Some(info))) = cache.get(&(scope.to_owned(), workspace.path.clone())) else {
            continue;
        };
        if workspace.project == workspace.path
            && let Some(project) = &info.project
        {
            workspace.project.clone_from(project);
        }
        if let Some(branch) = &info.branch {
            workspace.branch = Some(branch.clone());
        }
        // A finished probe is authoritative: no remote now means no repo.
        workspace.repo.clone_from(&info.repo);
    }
}

/// [`enrich`] through a remote shell: `run` executes a script there and returns
/// its stdout, or `None` when it could not.
pub(crate) fn enrich_remote(
    scope: &str,
    lists: &mut [&mut Vec<SessionWorkspace>],
    run: impl FnOnce(&str) -> Option<Vec<u8>>,
) {
    enrich(scope, lists, |paths| {
        let paths = fit(paths);
        // Nothing to ask means no SSH round trip at all.
        if paths.is_empty() {
            return Found::new();
        }
        run(&remote_script(paths))
            .map(|output| parse_remote(paths, &output))
            .unwrap_or_default()
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
