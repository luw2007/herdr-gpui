//! From a saved workspace the owning device probed to the identity of the pull
//! request lookup that answers for it. Nothing here does I/O.
//!
//! The identity is provider, host, base repository, head repository, head
//! branch and the account that reads it. A branch name alone is not an
//! identity: two forks can each have a `fix` branch with its own pull request.

use herdr_client::{HeadState, RepoKey, SessionWorkspace, UpstreamState};

/// What a device can be asked about in this version: this machine and SSH
/// hosts (their git runs where the checkout is). Other transports are
/// reported as unsupported, never as having no pull request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Support {
    Yes,
    No,
}

/// Why a workspace cannot be asked about right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Skip {
    /// The device has not been probed yet, or the probe failed.
    NotProbed,
    /// HEAD is detached: there is no branch to have a pull request.
    Detached,
    /// The checkout has no usable remote to name a repository.
    NoRemote,
    /// A fork checkout (`pr/<n>`) with no upstream to name its head.
    ForkCheckout,
    /// No GitHub account is signed in for the device.
    NoAccount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Provider {
    GitHub,
}

/// A repository on a host, as `owner/name`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Repo {
    pub owner: String,
    pub name: String,
}

/// The identity of one lookup, and the key of the shared cache.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Key {
    pub provider: Provider,
    pub host: String,
    /// The repository the pull request is opened against.
    pub base: Repo,
    /// The repository and branch the pull request is from.
    pub head: Repo,
    pub branch: String,
    /// The account the lookup reads with, so one account's invisible
    /// repository is never answered from another's cache.
    pub login: String,
}

/// What a workspace resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Resolution {
    Unsupported,
    Skip(Skip),
    Ask(Key),
}

const GITHUB: &str = "github.com";

/// `host/owner/name` split into its parts, for a two-segment path.
fn split(key: &RepoKey) -> Option<(&str, Repo)> {
    let mut parts = key.as_str().split('/');
    let (host, owner, name) = (parts.next()?, parts.next()?, parts.next()?);
    parts.next().is_none().then(|| {
        (
            host,
            Repo {
                owner: owner.to_owned(),
                name: name.to_owned(),
            },
        )
    })
}

/// The lookup that answers for `workspace`, read with `login` (`None` when no
/// account is signed in for its device).
pub(crate) fn resolve(
    workspace: &SessionWorkspace,
    support: Support,
    login: Option<&str>,
) -> Resolution {
    if support == Support::No {
        return Resolution::Unsupported;
    }
    // A remote that is not GitHub is a provider this version does not read,
    // which must never look like "no pull request".
    if let Some(repo) = &workspace.repo
        && split(repo).is_none_or(|(host, _)| host != GITHUB)
    {
        return Resolution::Unsupported;
    }
    match workspace.head {
        HeadState::Saved | HeadState::Unavailable => {
            return Resolution::Skip(Skip::NotProbed);
        }
        HeadState::Detached => return Resolution::Skip(Skip::Detached),
        HeadState::Branch => {}
    }
    let Some(branch) = workspace.branch.as_deref() else {
        return Resolution::Skip(Skip::NotProbed);
    };
    let Some((_, base)) = workspace.repo.as_ref().and_then(split) else {
        return Resolution::Skip(Skip::NoRemote);
    };
    // The head is the branch's upstream when it has one: the local name need
    // not be the pushed name, and the upstream remote may be a fork.
    // Only a branch known to have no upstream may be guessed to live on the
    // base repository under its local name; an upstream that could not be read
    // or cannot be followed would show an unrelated branch's pull request.
    let (head, branch) = match &workspace.upstream {
        UpstreamState::Known(upstream) => match split(&upstream.repo) {
            Some((host, repo)) if host == GITHUB => (repo, upstream.branch.clone()),
            _ => return Resolution::Unsupported,
        },
        UpstreamState::Unavailable => return Resolution::Skip(Skip::NotProbed),
        UpstreamState::Unsupported => return Resolution::Unsupported,
        UpstreamState::Absent if crate::repo_items::fork_branch_number(branch).is_some() => {
            return Resolution::Skip(Skip::ForkCheckout);
        }
        UpstreamState::Absent => (base.clone(), branch.to_owned()),
    };
    let Some(login) = login else {
        return Resolution::Skip(Skip::NoAccount);
    };
    Resolution::Ask(Key {
        provider: Provider::GitHub,
        host: GITHUB.to_owned(),
        base,
        head,
        branch,
        login: login.to_owned(),
    })
}
