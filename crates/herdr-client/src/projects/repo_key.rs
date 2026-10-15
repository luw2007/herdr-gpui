//! Repository identity from a git remote URL.
//!
//! A [`RepoKey`] is `host/path` in lowercase, e.g. `github.com/owner/repo`, so
//! the clones of one repository on different machines compare equal whatever
//! scheme, user, port, `.git` suffix, or letter case their remote uses. It
//! holds only that: userinfo (which can embed tokens) is discarded while
//! parsing and never stored.
//!
//! The whole key is lowercased, path included. GitHub, GitLab, Bitbucket and
//! Gitea resolve repository paths case-insensitively and forbid two
//! repositories differing only by case in one namespace, so folding joins the
//! spellings a user copies around; a case-sensitive self-hosted server holding
//! two such repositories is the accepted loss.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// The longest remote URL considered, in bytes.
const URL_MAX: usize = 1024;
/// The longest key kept, in bytes.
const KEY_MAX: usize = 512;

/// A normalized repository identity (`host/path`, lowercase).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepoKey(String);

impl RepoKey {
    /// The repository a remote URL names. Accepts `http`, `https`, `ssh` and
    /// `git` URLs and scp-style `user@host:owner/repo`; the user, password,
    /// port, query, fragment, trailing `/` and `.git` are dropped. Local
    /// paths, `file://`, other schemes, hosts or paths with unusual
    /// characters, a path of fewer than two segments, control characters and
    /// overlong input give `None`.
    pub fn from_remote_url(url: &str) -> Option<Self> {
        let url = url.trim();
        if url.is_empty() || url.len() > URL_MAX || url.chars().any(char::is_control) {
            return None;
        }
        let (host, path) = match url.split_once("://") {
            Some((scheme, rest)) => {
                if !["http", "https", "ssh", "git"]
                    .iter()
                    .any(|known| scheme.eq_ignore_ascii_case(known))
                {
                    return None;
                }
                let (authority, path) = rest.split_once('/')?;
                (host_of(authority, true)?, path)
            }
            None => {
                let (authority, path) = url.split_once(':')?;
                // `C:\repo` and `./dir:x` are local paths, not scp remotes.
                if authority.len() < 2 || authority.contains(['/', '\\']) {
                    return None;
                }
                (host_of(authority, false)?, path)
            }
        };
        let path = path.split(['?', '#']).next().unwrap_or_default();
        Self::build(&host, &normalize_path(path)?)
    }

    /// The key as `host/path`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn build(host: &str, path: &str) -> Option<Self> {
        let key = format!("{host}/{path}");
        (key.len() <= KEY_MAX).then_some(Self(key))
    }

    /// Accept a stored key only if it is already in normal form.
    fn parse(key: &str) -> Option<Self> {
        let (host, path) = key.split_once('/')?;
        let parsed = Self::build(&normalize_host(host)?, &normalize_path(path)?)?;
        (parsed.0 == key).then_some(parsed)
    }
}

impl fmt::Display for RepoKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for RepoKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RepoKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Self::parse(&key).ok_or_else(|| serde::de::Error::custom("invalid repository key"))
    }
}

/// The host of a URL authority, or of an scp-style `user@host`, lowercased
/// with the user and (for URLs) the port removed.
fn host_of(authority: &str, allow_port: bool) -> Option<String> {
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if let Some(rest) = host.strip_prefix('[') {
        let (inner, tail) = rest.split_once(']')?;
        if !allow_port && !tail.is_empty() {
            return None;
        }
        if !tail.is_empty() {
            port_ok(tail.strip_prefix(':')?)?;
        }
        return normalize_host(&format!("[{inner}]"));
    }
    let host = match host.split_once(':') {
        Some((host, port)) if allow_port => {
            port_ok(port)?;
            host
        }
        Some(_) => return None,
        None => host,
    };
    normalize_host(host)
}

fn port_ok(port: &str) -> Option<()> {
    (port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())).then_some(())
}

/// A lowercase DNS name, IPv4 address, or bracketed IPv6 address.
fn normalize_host(host: &str) -> Option<String> {
    let host = host.to_ascii_lowercase();
    let ok = match host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        Some(inner) => {
            !inner.is_empty()
                && inner
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() || matches!(b, b':' | b'.'))
        }
        None => {
            !host.is_empty()
                && host.len() <= 253
                && !host.starts_with('.')
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        }
    };
    ok.then_some(host)
}

/// The repository path: slashes trimmed, `.git` removed, at least two
/// non-empty segments (`owner/repo`, or nested groups), lowercased.
fn normalize_path(path: &str) -> Option<String> {
    let mut path = path.trim_matches('/');
    while let Some(stripped) = path.strip_suffix(".git") {
        path = stripped.trim_end_matches('/');
    }
    let path = path.to_lowercase();
    let mut segments = 0;
    for segment in path.split('/') {
        if segment.is_empty()
            || matches!(segment, "." | "..")
            || segment.contains(|c: char| c.is_whitespace() || matches!(c, '\\' | '?' | '#'))
        {
            return None;
        }
        segments += 1;
    }
    (segments >= 2).then_some(path)
}
