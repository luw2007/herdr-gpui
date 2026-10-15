use super::*;

#[test]
fn every_spelling_of_one_remote_normalizes_to_the_same_key() {
    for url in [
        "https://github.com/Owner/Repo.git",
        "https://github.com/owner/repo",
        "https://github.com/owner/repo/",
        "http://GitHub.com/owner/repo.git/",
        "https://user:secret@github.com/owner/repo.git",
        "https://x-access-token:ghp_abc@github.com:8443/owner/repo.git",
        "ssh://git@github.com/owner/repo.git",
        "ssh://git@github.com:22/owner/repo.git",
        "git://github.com/owner/repo.git",
        "git@github.com:owner/repo.git",
        "github.com:owner/repo",
        "  git@GITHUB.com:Owner/Repo.git\n",
        "https://github.com/owner/repo.git?tab=readme#top",
    ] {
        assert_eq!(
            RepoKey::from_remote_url(url).as_ref().map(RepoKey::as_str),
            Some("github.com/owner/repo"),
            "{url}"
        );
    }
}

#[test]
fn nested_namespaces_and_ip_hosts_are_kept() {
    assert_eq!(
        key("git@gitlab.example.com:group/sub/repo.git").as_str(),
        "gitlab.example.com/group/sub/repo"
    );
    assert_eq!(
        key("ssh://git@[2001:db8::1]:2222/o/r.git").as_str(),
        "[2001:db8::1]/o/r"
    );
    assert_eq!(key("https://10.0.0.5:3000/o/r").as_str(), "10.0.0.5/o/r");
}

#[test]
fn local_paths_and_unusable_urls_are_not_repositories() {
    let long = format!("https://github.com/o/{}", "r".repeat(2000));
    for url in [
        "",
        "   ",
        "/srv/git/repo.git",
        "../repo",
        "./repo.git",
        "C:\\work\\repo",
        "C:/work/repo",
        "file:///srv/git/repo.git",
        "ftp://example.com/o/r",
        "https://github.com",
        "https://github.com/",
        "https://github.com/onlyone",
        "https://github.com/o/../r",
        "git@github.com:repo",
        "git@:o/r",
        "host:",
        "https://git hub.com/o/r",
        "https://github.com/o/\u{7}r",
        "https://github.com:port/o/r",
        long.as_str(),
    ] {
        assert_eq!(RepoKey::from_remote_url(url), None, "{url:?}");
    }
}

#[test]
fn a_key_never_carries_credentials_or_ports() {
    let key = key("https://user:hunter2@github.com:8443/o/r.git");
    assert_eq!(key.as_str(), "github.com/o/r");
    assert!(!format!("{key:?}{key}").contains("hunter2"));
}

#[test]
fn deserialize_accepts_only_normal_form() {
    let ok: RepoKey = serde_json::from_str("\"github.com/o/r\"").unwrap();
    assert_eq!(ok, key("git@github.com:o/r.git"));
    assert_eq!(serde_json::to_string(&ok).unwrap(), "\"github.com/o/r\"");
    for bad in [
        "\"GitHub.com/o/r\"",
        "\"github.com/o/r.git\"",
        "\"github.com/o/r/\"",
        "\"github.com/o\"",
        "\"https://github.com/o/r\"",
        "\"user@github.com/o/r\"",
        "\"\"",
        "7",
    ] {
        assert!(serde_json::from_str::<RepoKey>(bad).is_err(), "{bad}");
    }
}
