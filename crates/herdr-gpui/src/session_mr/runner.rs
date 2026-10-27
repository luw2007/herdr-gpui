//! The real provider: GitHub through the same batched, fork-safe GraphQL path
//! the workspace menu uses, with the account each key names.

use super::{
    cache::Outcome,
    error::LookupError,
    key::Key,
    model::{Checks, Lifecycle, Pr, Review},
};
use crate::pull_request::{self, Resolved};
use secrecy::SecretString;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// One pull request as the cache keeps it. A lifecycle this client does not
/// know is an error, never an open pull request.
pub(super) fn convert(pr: &pull_request::PullRequest) -> Result<Pr, LookupError> {
    let lifecycle = match pr.state {
        pull_request::State::Open => Lifecycle::Open,
        pull_request::State::Merged => Lifecycle::Merged,
        pull_request::State::Closed => Lifecycle::Closed,
        pull_request::State::Unknown => return Err(LookupError::UnknownState),
    };
    let checks = match pr.checks_outcome() {
        Some(pull_request::Outcome::Failed) => Checks::Failing,
        Some(pull_request::Outcome::Pending) => Checks::Pending,
        Some(pull_request::Outcome::Passed) => Checks::Passing,
        Some(pull_request::Outcome::Skipped) | None => Checks::None,
    };
    let review = match pr.review_decision {
        pull_request::ReviewDecision::Approved => Review::Approved,
        pull_request::ReviewDecision::ChangesRequested => Review::ChangesRequested,
        pull_request::ReviewDecision::ReviewRequired => Review::Required,
        pull_request::ReviewDecision::None => Review::None,
    };
    Ok(Pr {
        number: pr.number,
        lifecycle,
        draft: pr.is_draft,
        checks,
        review,
    })
}

/// A lookup function reading with `accounts` (login to token). A key whose
/// account has no token fails alone.
pub(crate) fn github(
    accounts: BTreeMap<String, Arc<SecretString>>,
) -> impl FnOnce(Vec<Key>, Arc<AtomicBool>) -> Outcome + Send + 'static {
    move |keys, cancelled| {
        let stop = || cancelled.load(Ordering::Relaxed);
        let mut outcome = Outcome::default();
        let mut by_login: BTreeMap<&str, Vec<&Key>> = BTreeMap::new();
        for key in &keys {
            by_login.entry(key.login.as_str()).or_default().push(key);
        }
        for (login, keys) in by_login {
            let Some(token) = accounts.get(login) else {
                outcome.answers.extend(
                    keys.into_iter()
                        .map(|key| (key.clone(), Err(LookupError::NotSignedIn))),
                );
                continue;
            };
            let items: Vec<Resolved> = keys
                .iter()
                .map(|key| Resolved {
                    owner: key.base.owner.clone(),
                    repo: key.base.name.clone(),
                    head_owner: key.head.owner.clone(),
                    head_repo: key.head.name.clone(),
                    branch: key.branch.clone(),
                })
                .collect();
            let mut pause = None;
            let answers = pull_request::lookup_resolved(&items, token, stop, &mut pause);
            outcome.pause = outcome.pause.max(pause);
            for (index, result) in answers {
                let Some(key) = keys.get(index) else { continue };
                let result = result
                    .map_err(|source| LookupError::Github { source })
                    .and_then(|found| found.as_ref().map(convert).transpose());
                outcome.answers.push(((*key).clone(), result));
            }
            if stop() || outcome.pause.is_some() {
                break;
            }
        }
        outcome
    }
}
