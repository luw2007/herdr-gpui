//! The shared pull request cache behind MR grouping. One lookup per [`Key`]
//! however many sessions share it, one worker at a time, a bounded number of
//! keys per round, and a bounded table. A failed refresh keeps the last good
//! answer and marks it stale. Time and the provider are passed in, so the
//! cache is deterministic under test and never does I/O on its own.

use super::{
    error::LookupError,
    key::Key,
    model::{Answer, Freshness, Pr},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// Keys held at once, and the most one view may want.
pub(crate) const KEYS_MAX: usize = 256;
/// Keys one round asks the provider about.
pub(crate) const ROUND_MAX: usize = 40;
/// How long an open or unknown answer stands.
pub(crate) const REFRESH: Duration = Duration::from_secs(120);
/// How long a settled answer (merged, closed, or no pull request) stands.
pub(crate) const SETTLED: Duration = Duration::from_secs(600);
/// How long after a failure before the key is asked again.
pub(crate) const BACKOFF: Duration = Duration::from_secs(300);

/// What a round found.
#[derive(Default)]
pub(crate) struct Outcome {
    /// Each asked key's own answer or failure.
    pub answers: Vec<(Key, Result<Option<Pr>, LookupError>)>,
    /// The provider asked to be left alone this long.
    pub pause: Option<Duration>,
}

struct Entry {
    answer: Answer,
    /// Why the last refresh failed; `None` when it succeeded.
    error: Option<LookupError>,
    due: Instant,
    used: Instant,
}

struct Round {
    keys: Vec<Key>,
    done: mpsc::Receiver<Outcome>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<Key, Entry>,
    /// Wanted keys beyond [`KEYS_MAX`]: never asked, and said so.
    over: HashSet<Key>,
    round: Option<Round>,
    paused_until: Option<Instant>,
    /// Accounts another cache of this app has found paused, until when.
    held: Vec<(String, Instant)>,
}

impl Drop for Cache {
    /// A closed window must not leave its lookup running.
    fn drop(&mut self) {
        self.stop();
    }
}

impl Cache {
    /// The answer for `key`: `Unknown` until a lookup has answered, and
    /// `OverLimit` when the view wants more keys than the cache holds.
    pub(crate) fn answer(&self, key: &Key) -> Answer {
        match self.entries.get(key) {
            Some(entry) => entry.answer.clone(),
            None if self.over.contains(key) => Answer::OverLimit,
            None => Answer::Unknown,
        }
    }

    /// Accounts to leave alone until the given time, as another part of the
    /// app found them rate limited. Replaces the previous set.
    pub(crate) fn hold(&mut self, held: Vec<(String, Instant)>) {
        self.held = held;
    }

    /// Why `key`'s last refresh failed.
    #[cfg(test)]
    pub(crate) fn error(&self, key: &Key) -> Option<&LookupError> {
        self.entries.get(key)?.error.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn busy(&self) -> bool {
        self.round.is_some()
    }

    /// Stops the round in flight and forgets that it was running. What the
    /// cache has found stays, bounded, for when the grouping comes back.
    pub(crate) fn stop(&mut self) {
        if let Some(round) = self.round.take() {
            round.cancelled.store(true, Ordering::Relaxed);
        }
    }

    /// Applies a finished round, and starts the next when some wanted key is
    /// due. `run` is given the keys and a flag that turns true when the round
    /// is no longer wanted. Reports whether any answer changed.
    pub(crate) fn poll(
        &mut self,
        wanted: &BTreeSet<Key>,
        now: Instant,
        run: impl FnOnce(Vec<Key>, Arc<AtomicBool>) -> Outcome + Send + 'static,
    ) -> bool {
        let mut changed = false;
        if let Some(round) = &self.round {
            match round.done.try_recv() {
                Ok(outcome) => {
                    let round = self.round.take();
                    changed = self.apply(round.map(|round| round.keys), outcome, now);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    tracing::warn!("The sidebar's pull request lookup stopped unexpectedly");
                    if let Some(round) = self.round.take() {
                        self.defer(&round.keys, now + BACKOFF);
                    }
                }
            }
        }
        self.admit(wanted, now);
        if self.round.is_none() && self.paused_until.is_none_or(|until| now >= until) {
            self.start(wanted, now, run);
        }
        changed
    }

    fn apply(&mut self, asked: Option<Vec<Key>>, outcome: Outcome, now: Instant) -> bool {
        let mut changed = false;
        if let Some(pause) = outcome.pause {
            self.paused_until = Some(now + pause);
        }
        let mut answered = BTreeSet::new();
        for (key, result) in outcome.answers {
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            answered.insert(key);
            match result {
                Ok(found) => {
                    let settled = found
                        .as_ref()
                        .is_none_or(|pr| pr.lifecycle != super::model::Lifecycle::Open);
                    let next = Answer::Known(found, Freshness::Fresh);
                    changed |= entry.answer != next || entry.error.is_some();
                    entry.answer = next;
                    entry.error = None;
                    entry.due = now + if settled { SETTLED } else { REFRESH };
                }
                Err(message) => {
                    // The last good value stands, marked stale.
                    if let Answer::Known(found, Freshness::Fresh) = &entry.answer {
                        entry.answer = Answer::Known(found.clone(), Freshness::Stale);
                        changed = true;
                    }
                    entry.error = Some(message);
                    entry.due = now + BACKOFF;
                }
            }
        }
        // Keys the round never got to (a pause) wait out the pause, not
        // the failure backoff, and are not marked failed.
        let unasked: Vec<Key> = asked
            .into_iter()
            .flatten()
            .filter(|key| !answered.contains(key))
            .collect();
        let until = self.paused_until.unwrap_or(now + BACKOFF);
        self.defer(&unasked, until);
        changed
    }

    fn defer(&mut self, keys: &[Key], until: Instant) {
        for key in keys {
            if let Some(entry) = self.entries.get_mut(key) {
                entry.due = until;
            }
        }
    }

    /// Makes room for `wanted`, dropping the least recently wanted entries
    /// that are not wanted now. The cache holds at most [`KEYS_MAX`] keys: a
    /// view that wants more keeps the first [`KEYS_MAX`] in key order, and the
    /// rest are remembered in `over` so they read as [`Answer::OverLimit`]
    /// instead of an `Unknown` that would look like a lookup still on its way.
    fn admit(&mut self, wanted: &BTreeSet<Key>, now: Instant) {
        self.over.clear();
        for (position, key) in wanted.iter().enumerate() {
            if position >= KEYS_MAX {
                self.over.insert(key.clone());
                continue;
            }
            self.entries
                .entry(key.clone())
                .and_modify(|entry| entry.used = now)
                .or_insert(Entry {
                    answer: Answer::Unknown,
                    error: None,
                    due: now,
                    used: now,
                });
        }
        if self.entries.len() > KEYS_MAX {
            let in_flight: &[Key] = self.round.as_ref().map_or(&[], |round| &round.keys);
            let mut idle: Vec<(Instant, Key)> = self
                .entries
                .iter()
                .filter(|(key, _)| !wanted.contains(*key) && !in_flight.contains(key))
                .map(|(key, entry)| (entry.used, key.clone()))
                .collect();
            idle.sort();
            let extra = self.entries.len() - KEYS_MAX;
            for (_, key) in idle.into_iter().take(extra) {
                self.entries.remove(&key);
            }
        }
    }

    /// Whether `key`'s account is held back by a pause another part of the app
    /// found.
    fn held(&self, key: &Key, now: Instant) -> bool {
        self.held
            .iter()
            .any(|(login, until)| *login == key.login && now < *until)
    }
    fn start(
        &mut self,
        wanted: &BTreeSet<Key>,
        now: Instant,
        run: impl FnOnce(Vec<Key>, Arc<AtomicBool>) -> Outcome + Send + 'static,
    ) {
        let mut due: Vec<(Instant, &Key)> = wanted
            .iter()
            .filter_map(|key| {
                let entry = self.entries.get(key)?;
                (entry.due <= now && !self.held(key, now)).then_some((entry.due, key))
            })
            .collect();
        due.sort();
        let keys: Vec<Key> = due
            .into_iter()
            .take(ROUND_MAX)
            .map(|(_, key)| key.clone())
            .collect();
        if keys.is_empty() {
            return;
        }
        let (tx, done) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let asked = keys.clone();
        match std::thread::Builder::new()
            .name("herdr-gui-mr".into())
            .spawn(move || {
                let _ = tx.send(run(asked, flag));
            }) {
            Ok(_) => {
                self.round = Some(Round {
                    keys,
                    done,
                    cancelled,
                });
            }
            Err(error) => {
                tracing::warn!(%error, "Cannot start the sidebar's pull request lookup");
                self.defer(&keys, now + BACKOFF);
            }
        }
    }
}
