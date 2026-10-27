//! The MR badge of a session row: read from the cache, shown only while the
//! shown Window groups by MR, and only for its members.
use super::*;
use crate::{
    containers::Grouping,
    menu::sessions::Row,
    session_mr::{
        Outcome,
        model::{Checks, Lifecycle, Pr, Review},
    },
};
use herdr_client::{HeadState, RepoKey, SessionWorkspace};
use std::{sync::Arc, time::Instant};

fn feature() -> SessionWorkspace {
    SessionWorkspace {
        project: "/work/app".into(),
        path: "/work/app".into(),
        branch: Some("feature".into()),
        repo: RepoKey::from_remote_url("git@github.com:team/app.git"),
        head: HeadState::Branch,
        ..Default::default()
    }
}

fn catalog() -> Vec<LocalSession> {
    vec![
        session("default", SessionState::Running),
        session("work", SessionState::Running),
    ]
}

fn group(view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext, grouping: Grouping) {
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let shown = view.containers.shown_id();
            view.edit_containers(cx, |set| set.set_grouping(shown, grouping))
                .unwrap();
            cx.notify();
        })
    });
    draw(cx);
}

fn seed_and_look_up(view: &Entity<crate::HerdrWindow>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            let session = view.endpoints[0].session_ref();
            view.grouping.scan_mut().seed(session, vec![feature()]);
            view.menu.github = crate::github::Auth::connected_fixture();
            let mut run = Some(|keys: Vec<crate::session_mr::Key>, _: Arc<_>| Outcome {
                answers: keys
                    .into_iter()
                    .map(|key| {
                        let pr = Pr {
                            number: 42,
                            lifecycle: Lifecycle::Open,
                            draft: false,
                            checks: Checks::Failing,
                            review: Review::Approved,
                        };
                        (key, Ok(Some(pr)))
                    })
                    .collect(),
                pause: None,
            });
            for _ in 0..400 {
                match run.take() {
                    Some(run) => view.poll_mr_with(Instant::now(), run),
                    None => view.poll_mr_with(Instant::now(), |_, _| Outcome::default()),
                };
                if !view.grouping.mr_busy() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        })
    });
}

fn badge_text(
    view: &Entity<crate::HerdrWindow>,
    cx: &mut VisualTestContext,
    name: &str,
) -> Option<String> {
    cx.update(|_, cx| {
        let view = view.read(cx);
        view.sessions_row_badge(&Row::Local(name.into()))
    })
}

#[gpui::test]
fn no_badge_unless_the_shown_window_groups_by_mr(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    for grouping in [Grouping::None, Grouping::Project, Grouping::Status] {
        group(&view, cx, grouping);
        assert!(cx.debug_bounds("sessions-mr-0").is_none(), "{grouping:?}");
        assert_eq!(badge_text(&view, cx, "default"), None);
    }
}

#[gpui::test]
fn an_unasked_member_is_unknown_never_no_pr_and_a_non_member_has_no_badge(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    group(&view, cx, Grouping::Mr);
    let text = badge_text(&view, cx, "default").unwrap();
    assert!(text.starts_with("Unknown"), "{text}");
    assert!(!text.contains("No pull request"), "{text}");
    assert!(cx.debug_bounds("sessions-mr-0").is_some());
    assert_eq!(badge_text(&view, cx, "work"), None, "not a member");
    assert!(cx.debug_bounds("sessions-mr-1").is_none());
}

#[gpui::test]
fn a_looked_up_member_shows_its_bucket_and_number_inside_its_row(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    group(&view, cx, Grouping::Mr);
    seed_and_look_up(&view, cx);
    cx.update(|_, cx| view.update(cx, |_, cx| cx.notify()));
    draw(cx);
    assert_eq!(
        badge_text(&view, cx, "default").as_deref(),
        Some("Checks failing #42")
    );
    let row = bounds(cx, "sessions-row-0");
    let badge = bounds(cx, "sessions-mr-0");
    assert!(badge.left() >= row.left() && badge.right() <= row.right());
    assert!(badge.top() >= row.top() && badge.bottom() <= row.bottom());
}
#[cfg(feature = "cloud")]
#[gpui::test]
fn a_cloud_member_without_a_scan_badges_unsupported_not_unknown(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, catalog(), development("default"));
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.endpoints[0].connection.target = ConnectTarget::Cloud {
                provider: crate::cloud::CloudProvider::Coder,
                account: "https://coder.example.com".into(),
                id: "w1".into(),
                machine: "herdr-tidy-lark-aa".into(),
                session: "default".into(),
            };
        })
    });
    group(&view, cx, Grouping::Mr);
    let text = badge_text(&view, cx, "default").unwrap();
    assert!(text.starts_with("Unsupported"), "{text}");
}
