#![allow(clippy::unwrap_used)]

use super::{
    cache::{BACKOFF, KEYS_MAX, REFRESH, ROUND_MAX},
    error::LookupError,
    key::{Provider, Repo, Skip},
    model::{Answer, Checks, Freshness, Lifecycle, Pr, Review},
    *,
};
use herdr_client::{HeadState, RepoKey, SessionWorkspace, Upstream, UpstreamState};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

fn repo(url: &str) -> Option<RepoKey> {
    RepoKey::from_remote_url(url)
}

fn workspace(branch: Option<&str>, head: HeadState) -> SessionWorkspace {
    SessionWorkspace {
        project: "/w".into(),
        path: "/w".into(),
        branch: branch.map(Into::into),
        repo: repo("git@github.com:o/r.git"),
        head,
        ..Default::default()
    }
}

fn on(branch: &str) -> SessionWorkspace {
    workspace(Some(branch), HeadState::Branch)
}

pub(crate) fn pr(number: u64, lifecycle: Lifecycle) -> Pr {
    Pr {
        number,
        lifecycle,
        draft: false,
        checks: Checks::None,
        review: Review::None,
    }
}

fn key_of(workspace: &SessionWorkspace) -> Key {
    match resolve(workspace, Support::Yes, Some("me")) {
        Resolution::Ask(key) => key,
        other => panic!("not asked: {other:?}"),
    }
}

#[test]
fn a_detached_head_and_a_failed_probe_are_different_answers() {
    let detached = workspace(None, HeadState::Detached);
    let failed = workspace(None, HeadState::Unavailable);
    let unprobed = workspace(Some("saved"), HeadState::Saved);
    let resolve = |w| resolve(w, Support::Yes, Some("me"));
    assert_eq!(resolve(&detached), Resolution::Skip(Skip::Detached));
    assert_eq!(resolve(&failed), Resolution::Skip(Skip::NotProbed));
    // A saved branch nobody confirmed is not asked either.
    assert_eq!(resolve(&unprobed), Resolution::Skip(Skip::NotProbed));
    // A detached checkout has no PR to find: it is not "unknown".
    let mr = Mr::default().derive(&Input {
        workspaces: &[detached],
        support: Support::Yes,
        login: Some("me"),
    });
    assert_eq!(mr.bucket, Bucket::Unknown);
    assert_eq!(
        mr.workspaces,
        [WorkspaceMr::Skipped(Skip::Detached)],
        "kept distinct from a probe failure"
    );
}

#[test]
fn a_fork_with_the_same_branch_name_is_a_different_lookup() {
    let own = on("fix");
    let mut fork = on("fix");
    fork.upstream = UpstreamState::Known(Upstream {
        repo: repo("https://github.com/someone/r.git").unwrap(),
        branch: "fix".into(),
    });
    let (a, b) = (key_of(&own), key_of(&fork));
    assert_ne!(a, b);
    assert_eq!(
        a.head,
        Repo {
            owner: "o".into(),
            name: "r".into()
        }
    );
    assert_eq!(
        b.head,
        Repo {
            owner: "someone".into(),
            name: "r".into()
        }
    );
    assert_eq!(b.base, a.base);
    // The pushed name, not the local one, names the head.
    let mut renamed = on("local-name");
    renamed.upstream = UpstreamState::Known(Upstream {
        repo: repo("git@github.com:o/r.git").unwrap(),
        branch: "pushed".into(),
    });
    assert_eq!(key_of(&renamed).branch, "pushed");
    // Another account never reads another's cached answer.
    let other = resolve(&own, Support::Yes, Some("you"));
    assert_ne!(Resolution::Ask(a), other);
}

#[test]
fn only_github_on_supported_devices_is_asked_and_the_rest_is_unsupported() {
    let gitlab = SessionWorkspace {
        repo: repo("git@gitlab.com:o/r.git"),
        ..on("main")
    };
    assert_eq!(
        resolve(&gitlab, Support::Yes, Some("me")),
        Resolution::Unsupported
    );
    assert_eq!(
        resolve(&on("main"), Support::No, Some("me")),
        Resolution::Unsupported
    );
    let mr = Mr::default().derive(&Input {
        workspaces: &[gitlab],
        support: Support::Yes,
        login: Some("me"),
    });
    assert_eq!(mr.bucket, Bucket::Unsupported);
    assert_ne!(mr.bucket, Bucket::NoPr);
}

#[test]
fn a_fork_checkout_without_an_upstream_and_a_missing_remote_are_skipped() {
    assert_eq!(
        resolve(&on("pr/12"), Support::Yes, Some("me")),
        Resolution::Skip(Skip::ForkCheckout)
    );
    let bare = SessionWorkspace {
        repo: None,
        ..on("main")
    };
    assert_eq!(
        resolve(&bare, Support::Yes, Some("me")),
        Resolution::Skip(Skip::NoRemote)
    );
    assert_eq!(
        resolve(&on("main"), Support::Yes, None),
        Resolution::Skip(Skip::NoAccount)
    );
}

fn full(lifecycle: Lifecycle, draft: bool, checks: Checks, review: Review) -> Bucket {
    Pr {
        number: 1,
        lifecycle,
        draft,
        checks,
        review,
    }
    .bucket()
}

#[test]
fn the_bucket_priority_is_failing_changes_draft_pending_review_approved_open() {
    use Lifecycle::{Closed, Merged, Open};
    let all = [
        full(Open, true, Checks::Failing, Review::ChangesRequested),
        full(Open, true, Checks::Pending, Review::ChangesRequested),
        full(Open, true, Checks::Pending, Review::Required),
        full(Open, false, Checks::Pending, Review::Required),
        full(Open, false, Checks::Passing, Review::Required),
        full(Open, false, Checks::Passing, Review::Approved),
        full(Open, false, Checks::Passing, Review::None),
    ];
    assert_eq!(
        all,
        [
            Bucket::ChecksFailing,
            Bucket::ChangesRequested,
            Bucket::Draft,
            Bucket::ChecksPending,
            Bucket::ReviewRequired,
            Bucket::Approved,
            Bucket::Open,
        ]
    );
    assert_eq!(
        full(Merged, true, Checks::Failing, Review::Required),
        Bucket::Merged
    );
    assert_eq!(
        full(Closed, false, Checks::None, Review::None),
        Bucket::Closed
    );
    // The declaration order is the group order.
    let order = [
        Bucket::ChecksFailing,
        Bucket::ChangesRequested,
        Bucket::Draft,
        Bucket::ChecksPending,
        Bucket::ReviewRequired,
        Bucket::Approved,
        Bucket::Open,
        Bucket::NoPr,
        Bucket::Merged,
        Bucket::Closed,
        Bucket::Unknown,
        Bucket::Unsupported,
    ];
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
}

fn looked(found: Option<Pr>, freshness: Freshness) -> WorkspaceMr {
    WorkspaceMr::Looked(Answer::Known(found, freshness))
}

#[test]
fn several_workspaces_keep_all_and_one_summary_that_is_never_merged_while_any_is_open() {
    let merged = looked(Some(pr(1, Lifecycle::Merged)), Freshness::Fresh);
    let open = looked(Some(pr(2, Lifecycle::Open)), Freshness::Fresh);
    let none = looked(None, Freshness::Fresh);
    let unknown = WorkspaceMr::Looked(Answer::Unknown);

    let mixed = SessionMr::of(vec![merged.clone(), open.clone()]);
    assert_eq!(mixed.bucket, Bucket::Open);
    assert!(mixed.mixed);
    assert_eq!(mixed.workspaces.len(), 2);
    assert_eq!(mixed.numbers, [1, 2]);

    // A workspace not yet known could be open: not merged, not no-pr.
    let pending = SessionMr::of(vec![merged.clone(), unknown.clone()]);
    assert_eq!(pending.bucket, Bucket::Unknown);
    assert_eq!(
        SessionMr::of(vec![none.clone(), unknown]).bucket,
        Bucket::Unknown
    );

    let settled = SessionMr::of(vec![merged.clone(), merged.clone()]);
    assert_eq!((settled.bucket, settled.mixed), (Bucket::Merged, false));
    assert_eq!(
        SessionMr::of(vec![merged, none]).bucket,
        Bucket::NoPr,
        "a workspace with no PR outranks a merged one"
    );
    assert_eq!(SessionMr::of(Vec::new()).bucket, Bucket::Unknown);
}
#[test]
fn an_unsupported_workspace_keeps_a_session_from_reading_settled_but_not_from_an_open_pr() {
    let at = |lifecycle| looked(Some(pr(1, lifecycle)), Freshness::Fresh);
    let failing = looked(
        Some(Pr {
            checks: Checks::Failing,
            ..pr(2, Lifecycle::Open)
        }),
        Freshness::Fresh,
    );
    let unsupported = || WorkspaceMr::Unsupported;
    for settled in [
        at(Lifecycle::Merged),
        at(Lifecycle::Closed),
        looked(None, Freshness::Fresh),
    ] {
        for order in [
            vec![settled.clone(), unsupported()],
            vec![unsupported(), settled.clone()],
        ] {
            let summary = SessionMr::of(order);
            assert_eq!(summary.bucket, Bucket::Unsupported, "{settled:?}");
            assert!(summary.mixed);
        }
    }
    assert_eq!(
        SessionMr::of(vec![failing.clone(), unsupported()]).bucket,
        Bucket::ChecksFailing
    );
    assert_eq!(
        SessionMr::of(vec![unsupported(), at(Lifecycle::Open)]).bucket,
        Bucket::Open
    );
    // Not yet known and over the limit still outrank unsupported.
    let unknown = WorkspaceMr::Looked(Answer::Unknown);
    assert_eq!(
        SessionMr::of(vec![at(Lifecycle::Merged), unsupported(), unknown]).bucket,
        Bucket::Unknown
    );
    assert_eq!(
        SessionMr::of(vec![unsupported(), unsupported()]).bucket,
        Bucket::Unsupported
    );
}
#[test]
fn a_device_without_support_is_unsupported_even_with_no_workspaces() {
    let mr = Mr::default();
    for workspaces in [Vec::new(), vec![on("feature")]] {
        let summary = mr.derive(&Input {
            workspaces: &workspaces,
            support: Support::No,
            login: Some("me"),
        });
        assert_eq!(summary.bucket, Bucket::Unsupported);
        assert_eq!(summary.workspaces.len(), workspaces.len());
    }
    assert_eq!(
        derive(&mr, &[]).bucket,
        Bucket::Unknown,
        "supported, unscanned"
    );
}

fn pump(
    mr: &mut Mr,
    sessions: &[SessionWorkspace],
    now: Instant,
    run: impl FnOnce(Vec<Key>, Arc<AtomicBool>) -> Outcome + Send + 'static,
) -> bool {
    mr.poll(
        [Input {
            workspaces: sessions,
            support: Support::Yes,
            login: Some("me"),
        }],
        now,
        run,
    )
}

/// Runs polls until the round in flight has landed.
fn land(
    mr: &mut Mr,
    sessions: &[SessionWorkspace],
    now: Instant,
    first: impl FnOnce(Vec<Key>, Arc<AtomicBool>) -> Outcome + Send + 'static,
) -> bool {
    let mut changed = pump(mr, sessions, now, first);
    for _ in 0..500 {
        if !mr.cache().busy() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
        changed |= pump(mr, sessions, now, |_, _| Outcome::default());
    }
    changed
}

fn answer(found: Option<Pr>) -> impl FnOnce(Vec<Key>, Arc<AtomicBool>) -> Outcome {
    move |keys, _| Outcome {
        answers: keys
            .into_iter()
            .map(|key| (key, Ok(found.clone())))
            .collect(),
        pause: None,
    }
}

fn derive(mr: &Mr, workspaces: &[SessionWorkspace]) -> SessionMr {
    mr.derive(&Input {
        workspaces,
        support: Support::Yes,
        login: Some("me"),
    })
}

#[test]
fn sessions_on_the_same_head_share_one_lookup() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (record, seen) = mpsc::channel::<usize>();
    let counted = calls.clone();
    let mut mr = Mr::default();
    let now = Instant::now();
    let workspaces = [on("feature"), on("feature")];
    let wanted = [
        Input {
            workspaces: &workspaces[..1],
            support: Support::Yes,
            login: Some("me"),
        },
        Input {
            workspaces: &workspaces[1..],
            support: Support::Yes,
            login: Some("me"),
        },
    ];
    mr.poll(wanted, now, move |keys, _| {
        counted.fetch_add(1, Ordering::Relaxed);
        record.send(keys.len()).unwrap();
        Outcome {
            answers: keys
                .into_iter()
                .map(|key| (key, Ok(Some(pr(7, Lifecycle::Open)))))
                .collect(),
            pause: None,
        }
    });
    while mr.cache().busy() {
        std::thread::sleep(Duration::from_millis(5));
        mr.poll(Vec::<Input<'_>>::new(), now, |_, _| Outcome::default());
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        seen.try_iter().collect::<Vec<_>>(),
        [1],
        "one key for both sessions"
    );
    assert_eq!(mr.cache().len(), 1);
}

#[test]
fn a_failed_refresh_keeps_the_last_good_answer_marked_stale() {
    let mut mr = Mr::default();
    let ws = [on("feature")];
    let start = Instant::now();
    assert!(land(
        &mut mr,
        &ws,
        start,
        answer(Some(pr(3, Lifecycle::Open)))
    ));
    let fresh = derive(&mr, &ws);
    assert_eq!((fresh.bucket, fresh.stale), (Bucket::Open, false));

    let later = start + REFRESH + Duration::from_secs(1);
    land(&mut mr, &ws, later, |keys, _| Outcome {
        answers: keys
            .into_iter()
            .map(|key| (key, Err(LookupError::NotSignedIn)))
            .collect(),
        pause: None,
    });
    let stale = derive(&mr, &ws);
    assert_eq!(stale.bucket, Bucket::Open, "last good value kept");
    assert!(stale.stale);
    assert!(matches!(
        mr.cache().error(&key_of(&ws[0])),
        Some(LookupError::NotSignedIn)
    ));

    // Inside the backoff nothing is asked again.
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = asked.clone();
    land(&mut mr, &ws, later + Duration::from_secs(1), move |_, _| {
        counted.fetch_add(1, Ordering::Relaxed);
        Outcome::default()
    });
    assert_eq!(asked.load(Ordering::Relaxed), 0);

    // A later success clears the mark.
    land(
        &mut mr,
        &ws,
        later + BACKOFF + Duration::from_secs(1),
        answer(Some(pr(3, Lifecycle::Merged))),
    );
    let healed = derive(&mr, &ws);
    assert_eq!((healed.bucket, healed.stale), (Bucket::Merged, false));
}

#[test]
fn a_failure_before_any_answer_is_unknown_not_no_pr() {
    let mut mr = Mr::default();
    let ws = [on("feature")];
    land(&mut mr, &ws, Instant::now(), |keys, _| Outcome {
        answers: keys
            .into_iter()
            .map(|key| (key, Err(LookupError::UnknownState)))
            .collect(),
        pause: None,
    });
    assert_eq!(derive(&mr, &ws).bucket, Bucket::Unknown);
}

#[test]
fn a_provider_pause_holds_back_every_later_round() {
    let mut mr = Mr::default();
    let ws = [on("a")];
    let start = Instant::now();
    land(&mut mr, &ws, start, |_, _| Outcome {
        answers: Vec::new(),
        pause: Some(Duration::from_secs(900)),
    });
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = asked.clone();
    land(
        &mut mr,
        &ws,
        start + Duration::from_secs(600),
        move |_, _| {
            counted.fetch_add(1, Ordering::Relaxed);
            Outcome::default()
        },
    );
    assert_eq!(asked.load(Ordering::Relaxed), 0);
    assert_eq!(derive(&mr, &ws).bucket, Bucket::Unknown);
}

#[test]
fn rounds_and_the_cache_are_bounded() {
    let many: Vec<SessionWorkspace> = (0..KEYS_MAX + 50)
        .map(|n| on(&format!("branch-{n:04}")))
        .collect();
    let (record, asked) = mpsc::channel::<usize>();
    let mut mr = Mr::default();
    let now = Instant::now();
    pump(&mut mr, &many, now, move |keys, _| {
        record.send(keys.len()).unwrap();
        Outcome::default()
    });
    while mr.cache().busy() {
        std::thread::sleep(Duration::from_millis(5));
        mr.poll(Vec::<Input<'_>>::new(), now, |_, _| Outcome::default());
    }
    assert_eq!(asked.try_iter().collect::<Vec<_>>(), [ROUND_MAX]);
    assert!(mr.cache().len() <= KEYS_MAX);
    // Wanting different keys later evicts the old ones instead of growing.
    let other: Vec<SessionWorkspace> = (0..KEYS_MAX).map(|n| on(&format!("other-{n}"))).collect();
    pump(&mut mr, &other, now + Duration::from_secs(1), |_, _| {
        Outcome::default()
    });
    assert!(mr.cache().len() <= KEYS_MAX);
}

#[test]
fn stopping_cancels_the_round_in_flight() {
    let (send, flag) = mpsc::channel();
    let mut mr = Mr::default();
    let ws = [on("a")];
    pump(&mut mr, &ws, Instant::now(), move |_, cancelled| {
        send.send(cancelled.clone()).unwrap();
        // Ends only because the flag turns true.
        while !cancelled.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Outcome::default()
    });
    let cancelled = flag.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!cancelled.load(Ordering::Relaxed));
    mr.stop();
    assert!(!mr.cache().busy());
    assert!(cancelled.load(Ordering::Relaxed));
}

#[test]
fn the_provider_and_host_are_part_of_the_key() {
    let key = key_of(&on("a"));
    assert_eq!(
        (key.provider, key.host.as_str()),
        (Provider::GitHub, "github.com")
    );
    let wanted: BTreeSet<Key> = [key.clone(), key].into_iter().collect();
    assert_eq!(wanted.len(), 1);
}

mod runner {
    use super::super::runner::convert;
    use super::*;
    use crate::pull_request::fixture;

    #[test]
    fn a_real_pull_request_maps_its_separate_fields() {
        let found = convert(&fixture().unwrap()).unwrap();
        assert_eq!(found.number, 8);
        assert_eq!(found.lifecycle, Lifecycle::Open);
        assert!(!found.draft);
        assert_eq!(found.review, Review::Required);
        assert_eq!(found.checks, Checks::Failing);
        assert_eq!(found.bucket(), Bucket::ChecksFailing);
    }
}

/// Runs until told to stop, then reports that it did. Bounded, so a missing
/// cancellation fails the test instead of leaking a spinning thread.
fn until_cancelled(
    done: mpsc::Sender<()>,
) -> impl FnOnce(Vec<Key>, Arc<AtomicBool>) -> Outcome + Send {
    move |_, cancelled| {
        for _ in 0..2000 {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = done.send(());
        Outcome::default()
    }
}

#[test]
fn dropping_the_cache_stops_the_round_in_flight() {
    let (done, exited) = mpsc::channel();
    let mut mr = Mr::default();
    pump(
        &mut mr,
        &[on("feature")],
        Instant::now(),
        until_cancelled(done),
    );
    assert!(mr.cache().busy());
    drop(mr);
    assert!(
        exited.recv_timeout(Duration::from_secs(5)).is_ok(),
        "the worker outlived its cache"
    );
}

#[test]
fn wanting_more_than_the_cache_holds_says_so_instead_of_unknown() {
    let many: Vec<SessionWorkspace> = (0..KEYS_MAX + 3)
        .map(|n| on(&format!("branch-{n:04}")))
        .collect();
    let mut mr = Mr::default();
    let now = Instant::now();
    pump(&mut mr, &many, now, |_, _| Outcome::default());
    let kept = derive(&mr, &many[..1]);
    assert_eq!(kept.bucket, Bucket::Unknown, "inside the limit: on its way");
    for over in &many[KEYS_MAX..] {
        let session = derive(&mr, std::slice::from_ref(over));
        assert_eq!(session.bucket, Bucket::OverLimit);
        assert_eq!(session.workspaces, [WorkspaceMr::Looked(Answer::OverLimit)]);
    }
    // A session with one workspace over the limit is never read as settled.
    let mixed = derive(&mr, &[many[0].clone(), many[KEYS_MAX].clone()]);
    assert_eq!(mixed.bucket, Bucket::OverLimit);
    // Once the view wants fewer keys, the overflow is asked like any other.
    pump(&mut mr, &many[KEYS_MAX..], now, |_, _| Outcome::default());
    assert_eq!(derive(&mr, &many[KEYS_MAX..]).bucket, Bucket::Unknown);
}

#[test]
fn a_pause_found_elsewhere_holds_back_that_account_only() {
    let mut mr = Mr::default();
    let ws = [on("feature")];
    let start = Instant::now();
    mr.hold(vec![("me".to_owned(), start + Duration::from_secs(60))]);
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = asked.clone();
    land(&mut mr, &ws, start, move |_, _| {
        counted.fetch_add(1, Ordering::Relaxed);
        Outcome::default()
    });
    assert_eq!(
        asked.load(Ordering::Relaxed),
        0,
        "held account is not asked"
    );
    let other = Arc::new(AtomicUsize::new(0));
    let counted = other.clone();
    mr.hold(vec![(
        "someone".to_owned(),
        start + Duration::from_secs(60),
    )]);
    land(&mut mr, &ws, start, move |_, _| {
        counted.fetch_add(1, Ordering::Relaxed);
        Outcome::default()
    });
    assert_eq!(other.load(Ordering::Relaxed), 1, "another account is");
    // The pause lapses by itself.
    let later = Arc::new(AtomicUsize::new(0));
    let counted = later.clone();
    let mut mr = Mr::default();
    mr.hold(vec![("me".to_owned(), start + Duration::from_secs(60))]);
    land(
        &mut mr,
        &ws,
        start + Duration::from_secs(61),
        move |_, _| {
            counted.fetch_add(1, Ordering::Relaxed);
            Outcome::default()
        },
    );
    assert_eq!(later.load(Ordering::Relaxed), 1);
}

#[test]
fn a_github_failure_reaches_the_cache_typed_with_its_source() {
    let ws = [on("feature")];
    let key = key_of(&ws[0]);
    let accounts = std::collections::BTreeMap::from([(
        "me".to_owned(),
        Arc::new(secrecy::SecretString::from("token".to_owned())),
    )]);
    // Cancelled before the first request: the lookup fails with its own error
    // and never reaches the network.
    let outcome = github(accounts)(vec![key.clone()], Arc::new(AtomicBool::new(true)));
    let [(asked, Err(error))] = outcome.answers.as_slice() else {
        panic!("expected one failed answer");
    };
    assert_eq!(*asked, key);
    let LookupError::Github { source } = error else {
        panic!("not a GitHub failure: {error:?}");
    };
    assert!(matches!(**source, crate::Error::PrCancelled));
    let reported = std::error::Error::source(error).unwrap();
    assert!(reported.downcast_ref::<Arc<crate::Error>>().is_some());
    assert_eq!(reported.to_string(), "PR lookup cancelled.");

    // An account with no token fails alone, as its own variant.
    let outcome = github(Default::default())(vec![key], Arc::new(AtomicBool::new(false)));
    assert!(matches!(
        outcome.answers.as_slice(),
        [(_, Err(LookupError::NotSignedIn))]
    ));
}

#[test]
fn an_unknown_lifecycle_is_a_typed_error() {
    let mut unknown = crate::pull_request::fixture().unwrap();
    unknown.state = crate::pull_request::State::Unknown;
    assert!(matches!(
        super::runner::convert(&unknown),
        Err(LookupError::UnknownState)
    ));
}

#[test]
fn only_a_branch_known_to_have_no_upstream_falls_back_to_origin() {
    let mut unreadable = on("feature");
    unreadable.upstream = UpstreamState::Unavailable;
    assert_eq!(
        resolve(&unreadable, Support::Yes, Some("me")),
        Resolution::Skip(Skip::NotProbed)
    );
    let mut unsupported = on("feature");
    unsupported.upstream = UpstreamState::Unsupported;
    assert_eq!(
        resolve(&unsupported, Support::Yes, Some("me")),
        Resolution::Unsupported
    );
    // Absent still asks origin about the local name.
    assert_eq!(key_of(&on("feature")).branch, "feature");
}
