//! Pull request / merge request state per session, for grouping a Window's
//! members by it. The chain is: a session's saved workspaces (probed by the
//! device that owns them for real branch, detached HEAD, remote and upstream)
//! resolve to lookup [`Key`]s; one shared [`Cache`] answers each key once;
//! each session derives a [`SessionMr`] from the answers of its workspaces.
//!
//! Nothing is stored on the Window: sessions are the source, and everything
//! here is derived and read-only. Rendering only reads the cache. The lookup
//! runs on one background worker, only while a Window groups by MR.
//!
//! Only github.com on this machine and SSH devices is read. Other providers
//! and transports are `Unsupported`, which is never "no pull request".

mod cache;
mod error;
mod key;
pub(crate) mod model;
mod runner;

#[cfg(test)]
mod tests;

pub(crate) use {
    cache::{Cache, Outcome},
    key::{Key, Resolution, Support, resolve},
    model::{Bucket, SessionMr, WorkspaceMr},
    runner::github,
};

use herdr_client::SessionWorkspace;
use std::{
    collections::BTreeSet,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

/// The MR side of the window: the cache the sessions share.
#[derive(Default)]
pub(crate) struct Mr {
    cache: Cache,
}

/// A session as MR derivation sees it.
pub(crate) struct Input<'a> {
    pub workspaces: &'a [SessionWorkspace],
    pub support: Support,
    pub login: Option<&'a str>,
}

impl Mr {
    /// The summary for one session, from the cache only.
    pub(crate) fn derive(&self, input: &Input<'_>) -> SessionMr {
        // A device without a scan has no workspaces to resolve, yet it is
        // unsupported, not unknown: decide it before looking at workspaces.
        if input.support == Support::No {
            return SessionMr::unsupported(input.workspaces.len());
        }
        let items = input
            .workspaces
            .iter()
            .map(
                |workspace| match resolve(workspace, input.support, input.login) {
                    Resolution::Unsupported => WorkspaceMr::Unsupported,
                    Resolution::Skip(skip) => WorkspaceMr::Skipped(skip),
                    Resolution::Ask(key) => WorkspaceMr::Looked(self.cache.answer(&key)),
                },
            )
            .collect();
        SessionMr::of(items)
    }

    /// Keeps the lookups for `sessions` current: applies a finished round and
    /// starts at most one next. Reports whether an answer changed.
    pub(crate) fn poll<'a>(
        &mut self,
        sessions: impl IntoIterator<Item = Input<'a>>,
        now: Instant,
        run: impl FnOnce(Vec<Key>, Arc<AtomicBool>) -> Outcome + Send + 'static,
    ) -> bool {
        let wanted: BTreeSet<Key> = sessions
            .into_iter()
            .flat_map(|input| {
                input
                    .workspaces
                    .iter()
                    .filter_map(move |workspace| {
                        match resolve(workspace, input.support, input.login) {
                            Resolution::Ask(key) => Some(key),
                            _ => None,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        self.cache.poll(&wanted, now, run)
    }

    #[cfg(test)]
    pub(crate) fn busy(&self) -> bool {
        self.cache.busy()
    }

    /// Leaves `held` accounts alone until their times, as another reader of
    /// GitHub found them rate limited.
    pub(crate) fn hold(&mut self, held: Vec<(String, Instant)>) {
        self.cache.hold(held);
    }

    /// Stops the lookup in flight, as when the Window stops grouping by MR.
    pub(crate) fn stop(&mut self) {
        self.cache.stop();
    }

    #[cfg(test)]
    pub(crate) fn cache(&self) -> &Cache {
        &self.cache
    }
}
