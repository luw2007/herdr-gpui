//! Lookups whose repository and head are already known, so no checkout is
//! inspected here. The sidebar's MR grouping learns them from the owning
//! device's git probe; this runs the same batched, size-bounded, fork-safe
//! GraphQL path the workspace menu uses.

use super::{
    Input, PullRequest,
    batch::{BATCH_LIMIT, Batch, answer_for, ask},
    fetch::{Ask, Head, TIMEOUT, Target},
};
use std::{sync::Arc, time::Duration};

/// One pull request to find: the base repository, and the head it is from.
/// The head is always named, so a same-named branch of another fork is never
/// taken for it.
pub(crate) struct Resolved {
    pub owner: String,
    pub repo: String,
    pub head_owner: String,
    pub head_repo: String,
    pub branch: String,
}

/// One item's answer. The error is shared by every item one failed request
/// left unanswered.
pub(crate) type Answer = Result<Option<PullRequest>, Arc<crate::Error>>;

/// Each item's own answer, by its index in `items`. An item a pause or the
/// network left unasked is absent and stays due.
pub(crate) fn lookup_resolved(
    items: &[Resolved],
    token: &secrecy::SecretString,
    cancelled: impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> Vec<(usize, Answer)> {
    let mut batch = Batch::default();
    for chunk in items.chunks(BATCH_LIMIT).enumerate() {
        let (chunk_index, chunk) = chunk;
        let targets = chunk
            .iter()
            .enumerate()
            .map(|(offset, item)| {
                let index = chunk_index * BATCH_LIMIT + offset;
                (
                    Input {
                        checkout: None,
                        repo_key: Some(index.to_string()),
                        branch: item.branch.clone(),
                    },
                    Target {
                        owner: item.owner.clone(),
                        repo: item.repo.clone(),
                        ask: Ask::Branch {
                            branch: item.branch.clone(),
                            head: Some(Head {
                                owner: item.head_owner.clone(),
                                repo: item.head_repo.clone(),
                                branch: item.branch.clone(),
                            }),
                        },
                    },
                )
            })
            .collect();
        ask(
            targets,
            |query, variables, aliases, cooldown| {
                crate::github::graphql_aliases(
                    token, query, variables, aliases, TIMEOUT, &cancelled, cooldown,
                )
            },
            |response, target, pause| answer_for(response, target, token, &cancelled, pause),
            (&mut batch, cooldown),
        );
    }
    let index = |input: &Input| input.repo_key.as_deref()?.parse::<usize>().ok();
    let mut answers = Vec::with_capacity(items.len());
    for (input, result) in batch.found {
        if let Some(index) = index(&input) {
            answers.push((index, result.map_err(Arc::new)));
        }
    }
    for (inputs, error) in batch.failed {
        let error = Arc::new(error);
        answers.extend(
            inputs
                .iter()
                .filter_map(index)
                .map(|index| (index, Err(Arc::clone(&error)))),
        );
    }
    answers
}
