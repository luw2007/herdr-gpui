//! The window's side of MR grouping: which accounts and devices a session's
//! pull requests are read with, and the background lookup that runs only while
//! the shown Window groups by MR. Render reads the cache; this never blocks.

use crate::{
    HerdrWindow,
    session_mr::{self, Bucket, Input, SessionMr, Support},
};
use herdr_client::ConnectTarget;
use secrecy::SecretString;
use std::{collections::BTreeMap, sync::Arc, time::Instant};

impl HerdrWindow {
    /// Whether pull requests of the session behind endpoint `index` can be
    /// read: git runs where the checkout is, which this version reaches on
    /// this machine and over SSH. WSL and cloud machines are not covered.
    fn mr_support(&self, index: usize) -> Support {
        match &self.endpoints[index].connection.target {
            ConnectTarget::Ssh { .. }
            | ConnectTarget::Local
            | ConnectTarget::Session { .. }
            | ConnectTarget::Socket(_) => Support::Yes,
            _ => Support::No,
        }
    }

    /// The GitHub account the session's device is read with: the device's own
    /// sign-in when it has one, else the main account.
    fn mr_account(&self, index: usize) -> Option<(&str, &Arc<SecretString>)> {
        let endpoint = &self.endpoints[index];
        let own = matches!(endpoint.connection.target, ConnectTarget::Ssh { .. })
            .then(|| self.menu.github_hosts.get(endpoint.device_id()))
            .flatten()
            .and_then(|auth| auth.profile.as_ref());
        let profile = own.or(self.menu.github.profile.as_ref())?;
        Some((profile.login.as_str(), &profile.token))
    }

    /// The Window's sessions as MR derivation reads them: every endpoint it
    /// lists, with its cached saved workspaces.
    fn mr_inputs(&self) -> impl Iterator<Item = (usize, Input<'_>)> {
        let window = self.containers.active();
        (0..self.endpoints.len())
            .filter(move |&index| {
                let session = self.endpoints[index].session_ref();
                window.members.contains(&session)
            })
            .map(move |index| {
                let session = self.endpoints[index].session_ref();
                (
                    index,
                    Input {
                        workspaces: self.grouping.scan.workspaces(&session),
                        support: self.mr_support(index),
                        login: self.mr_account(index).map(|(login, _)| login),
                    },
                )
            })
    }

    /// The summary of the session behind endpoint `index`, from the cache.
    pub(crate) fn session_mr(&self, index: usize) -> SessionMr {
        let session = self.endpoints[index].session_ref();
        self.grouping.mr.derive(&Input {
            workspaces: self.grouping.scan.workspaces(&session),
            support: self.mr_support(index),
            login: self.mr_account(index).map(|(login, _)| login),
        })
    }

    pub(super) fn mr_bucket(&self, index: usize) -> Bucket {
        self.session_mr(index).bucket
    }

    /// Runs one lookup step with the real GitHub provider.
    pub(super) fn poll_mr(&mut self) -> bool {
        let accounts = self.mr_accounts();
        self.poll_mr_with(Instant::now(), session_mr::github(accounts))
    }

    fn mr_accounts(&self) -> BTreeMap<String, Arc<SecretString>> {
        self.mr_inputs()
            .filter_map(|(index, _)| self.mr_account(index))
            .map(|(login, token)| (login.to_owned(), token.clone()))
            .collect()
    }

    /// [`Self::poll_mr`] with the provider injected.
    pub(crate) fn poll_mr_with(
        &mut self,
        now: Instant,
        run: impl FnOnce(
            Vec<session_mr::Key>,
            Arc<std::sync::atomic::AtomicBool>,
        ) -> session_mr::Outcome
        + Send
        + 'static,
    ) -> bool {
        // Borrow-split: the inputs read `scan`, the cache is `mr`.
        let inputs: Vec<(Option<String>, Support, Vec<herdr_client::SessionWorkspace>)> = self
            .mr_inputs()
            .map(|(_, input)| {
                (
                    input.login.map(str::to_owned),
                    input.support,
                    input.workspaces.to_vec(),
                )
            })
            .collect();
        self.grouping.mr.poll(
            inputs.iter().map(|(login, support, workspaces)| Input {
                workspaces,
                support: *support,
                login: login.as_deref(),
            }),
            now,
            run,
        )
    }
}
