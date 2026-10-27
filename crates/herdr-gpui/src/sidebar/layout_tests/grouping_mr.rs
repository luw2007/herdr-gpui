//! MR grouping in the sidebar, with an injected provider and no network.
use super::grouping::{draw, group, open, painted};
use super::*;
use crate::{
    containers::Grouping,
    session_mr::{
        Bucket, Outcome,
        model::{Checks, Lifecycle, Pr, Review},
    },
};
use herdr_client::{HeadState, RepoKey, SessionWorkspace};
use std::{sync::Arc, time::Instant};

const FAILING: &str = "group-mr-checks-failing";
const APPROVED: &str = "group-mr-approved";
const NO_PR: &str = "group-mr-no-pr";
const UNKNOWN: &str = "group-mr-unknown";
const UNSUPPORTED: &str = "group-mr-unsupported";

fn on(branch: &str, remote: &str) -> SessionWorkspace {
    SessionWorkspace {
        project: "/work/app".into(),
        path: "/work/app".into(),
        branch: Some(branch.into()),
        repo: RepoKey::from_remote_url(remote),
        head: HeadState::Branch,
        ..Default::default()
    }
}

fn pr(number: u64, checks: Checks, review: Review) -> Pr {
    Pr {
        number,
        lifecycle: Lifecycle::Open,
        draft: false,
        checks,
        review,
    }
}

/// Answers by branch name. A branch the table lacks has no pull request.
fn provider(
    table: Vec<(&'static str, Pr)>,
) -> impl FnOnce(Vec<crate::session_mr::Key>, Arc<std::sync::atomic::AtomicBool>) -> Outcome + Send
{
    move |keys, _| Outcome {
        answers: keys
            .into_iter()
            .map(|key| {
                let found = table
                    .iter()
                    .find(|(branch, _)| *branch == key.branch)
                    .map(|(_, pr)| pr.clone());
                (key, Ok(found))
            })
            .collect(),
        pause: None,
    }
}

/// local: no PR. a: failing checks. b: approved. c: gitlab (unsupported).
fn seed(view: &mut HerdrWindow) {
    let github = "git@github.com:team/app.git";
    let sessions = [
        (0, vec![on("quiet", github)]),
        (1, vec![on("broken", github)]),
        (2, vec![on("ready", github)]),
        (3, vec![on("other", "git@gitlab.com:team/app.git")]),
    ];
    for (index, workspaces) in sessions {
        let session = view.endpoints[index].session_ref();
        view.grouping.scan_mut().seed(session, workspaces);
    }
    // The account the lookups read with.
    view.menu.github = crate::github::Auth::connected_fixture();
}

fn run_lookup(view: &mut HerdrWindow) {
    let table = vec![
        ("broken", pr(1, Checks::Failing, Review::Approved)),
        ("ready", pr(2, Checks::Passing, Review::Approved)),
    ];
    let mut run = Some(provider(table));
    for _ in 0..400 {
        view.poll_mr_with(
            Instant::now(),
            run.take().map_or_else(
                || Box::new(|_, _| Outcome::default()) as Box<dyn FnOnce(_, _) -> Outcome + Send>,
                |run| Box::new(run),
            ),
        );
        if !view.grouping.mr_busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[gpui::test]
fn mr_groups_paint_in_priority_order_and_unsupported_is_not_no_pr(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx, Grouping::None);
    view.update(cx, |view, cx| {
        seed(view);
        group(view, Grouping::Mr);
        run_lookup(view);
        cx.notify();
    });
    draw(cx);
    let all = [
        FAILING,
        APPROVED,
        NO_PR,
        UNKNOWN,
        UNSUPPORTED,
        "host-local",
        "host-ssh:a",
        "host-ssh:b",
        "host-ssh:c",
    ];
    assert_eq!(
        painted(cx, &all),
        [
            FAILING,
            "host-ssh:a",
            APPROVED,
            "host-ssh:b",
            NO_PR,
            "host-local",
            UNSUPPORTED,
            "host-ssh:c",
        ]
    );
}

#[gpui::test]
fn leaving_mr_grouping_stops_the_background_lookup(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx, Grouping::None);
    view.update(cx, |view, cx| {
        seed(view);
        group(view, Grouping::Mr);
        let (hold, release) = std::sync::mpsc::channel::<()>();
        view.poll_mr_with(Instant::now(), move |_, cancelled| {
            // Ends only when told to, or when the lookup is cancelled.
            while release.try_recv().is_err()
                && !cancelled.load(std::sync::atomic::Ordering::Relaxed)
            {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Outcome::default()
        });
        assert!(view.grouping.mr_busy());
        group(view, Grouping::Status);
        view.poll_grouping(cx);
        assert!(
            !view.grouping.mr_busy(),
            "the lookup stopped with the grouping"
        );
        drop(hold);
    });
}

#[gpui::test]
fn a_session_with_no_answer_yet_is_unknown_and_never_painted_as_no_pr(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = open(cx, Grouping::None);
    view.update(cx, |view, cx| {
        seed(view);
        group(view, Grouping::Mr);
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds(UNKNOWN).is_some());
    assert!(cx.debug_bounds(NO_PR).is_none());
    let bucket = cx.update(|_, cx| view.read(cx).session_mr(1).bucket);
    assert_eq!(bucket, Bucket::Unknown);
}
#[cfg(feature = "cloud")]
#[gpui::test]
fn a_cloud_session_without_a_scan_is_unsupported_in_its_group_not_unknown(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = open(cx, Grouping::None);
    view.update(cx, |view, cx| {
        view.endpoints[0].connection.target = ConnectTarget::Cloud {
            provider: crate::cloud::CloudProvider::Coder,
            account: "https://coder.example.com".into(),
            id: "w1".into(),
            machine: "herdr-tidy-lark-aa".into(),
            session: "default".into(),
        };
        group(view, Grouping::Mr);
        cx.notify();
    });
    draw(cx);
    let bucket = cx.update(|_, cx| view.read(cx).session_mr(0).bucket);
    assert_eq!(bucket, Bucket::Unsupported);
    assert!(cx.debug_bounds(UNSUPPORTED).is_some());
}
