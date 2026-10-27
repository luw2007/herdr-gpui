use super::*;
use crate::{containers::Grouping, state::ConnectionStatus};
use herdr_client::{Registry, RepoKey, SessionRef, SessionWorkspace};

const ALPHA_REPO: &str = "https://example.com/team/alpha.git";
const ALPHA_ID: &str = "00000000-0000-4000-8000-00000000a1fa";
const BETA_ID: &str = "00000000-0000-4000-8000-00000000be7a";
const ALPHA: &str = "group-project-00000000-0000-4000-8000-00000000a1fa";
const BETA: &str = "group-project-00000000-0000-4000-8000-00000000be7a";
const NONE: &str = "group-unclassified";
const ALPHA_COUNT: &str = "group-count-project-00000000-0000-4000-8000-00000000a1fa";

fn remote(id: &str, label: &str) -> crate::endpoint::Endpoint {
    let mut remote = crate::endpoint::Endpoint::new(
        id.into(),
        label.into(),
        ConnectTarget::Ssh {
            target: "unused".into(),
            session: "default".into(),
        },
        true,
    );
    remote.live.snapshot = Some(Arc::new(snapshot(1)));
    remote.live.status = ConnectionStatus::Connected;
    remote
}

fn workspace(repo: &str) -> SessionWorkspace {
    SessionWorkspace {
        project: "/work/app".into(),
        path: "/work/app".into(),
        branch: None,
        label: None,
        repo: RepoKey::from_remote_url(repo),
        ..Default::default()
    }
}

/// local, ssh:a, ssh:b, ssh:c in Window order. `a` and `b` hold the Alpha
/// repository on different devices, `c` is assigned to Beta, and `local` is
/// in no project. Grouping is off until a test turns it on.
fn build(window: &mut Window, cx: &mut Context<HerdrWindow>) -> HerdrWindow {
    let mut view = fixture_window(window, cx);
    view.live.snapshot = Some(Arc::new(snapshot(1)));
    view.live.status = ConnectionStatus::Connected;
    view.endpoints[0].live = view.live.clone();
    for (id, label) in [
        ("ssh:a", "Alpha one"),
        ("ssh:b", "Alpha two"),
        ("ssh:c", "Gamma"),
    ] {
        view.endpoints.push(remote(id, label));
    }
    view.list_all_endpoints();
    let mut registry = Registry::new();
    let alpha = registry
        .create_project("Alpha", RepoKey::from_remote_url(ALPHA_REPO))
        .unwrap();
    let beta = registry.create_project("Beta", None).unwrap();
    let c = view.endpoints[3].session_ref();
    registry.assign(c, &beta.id).unwrap();
    // Fixed ids let the heading selectors, which gpui wants as `'static`
    // text, be constants.
    let json = String::from_utf8(registry.to_json().unwrap())
        .unwrap()
        .replace(&alpha.id.to_string(), ALPHA_ID)
        .replace(&beta.id.to_string(), BETA_ID);
    view.sessions.projects = Registry::from_json(json.as_bytes()).unwrap();
    for index in [1, 2] {
        let session = view.endpoints[index].session_ref();
        view.grouping
            .scan_mut()
            .seed(session, vec![workspace(ALPHA_REPO)]);
    }
    view
}

pub(super) fn group(view: &mut HerdrWindow, grouping: Grouping) {
    let shown = view.containers.shown_id();
    view.containers
        .edit_local(|set| set.set_grouping(shown, grouping))
        .unwrap();
}

pub(super) fn open(
    cx: &mut gpui::TestAppContext,
    grouping: Grouping,
) -> (Entity<HerdrWindow>, &mut gpui::VisualTestContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = build(window, cx);
        group(&mut view, grouping);
        view
    });
    cx.simulate_resize(size(px(800.), px(700.)));
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

pub(super) fn draw(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
}

pub(super) fn top(cx: &mut gpui::VisualTestContext, selector: &'static str) -> Option<Pixels> {
    cx.debug_bounds(selector).map(|bounds| bounds.top())
}

/// The selectors in the order they are painted, top to bottom.
pub(super) fn painted(
    cx: &mut gpui::VisualTestContext,
    selectors: &[&'static str],
) -> Vec<&'static str> {
    let mut found: Vec<_> = selectors
        .iter()
        .filter_map(|selector| Some((top(cx, selector)?, *selector)))
        .collect();
    found.sort_by(|a, b| f32::from(a.0).total_cmp(&f32::from(b.0)));
    found.into_iter().map(|(_, selector)| selector).collect()
}

#[gpui::test]
fn sessions_of_one_project_share_a_group_across_devices_and_no_project_comes_last(
    cx: &mut gpui::TestAppContext,
) {
    let (_view, cx) = open(cx, Grouping::Project);
    let all = [
        ALPHA,
        BETA,
        NONE,
        "host-local",
        "host-ssh:a",
        "host-ssh:b",
        "host-ssh:c",
    ];
    assert_eq!(
        painted(cx, &all),
        [
            ALPHA,
            "host-ssh:a",
            "host-ssh:b",
            BETA,
            "host-ssh:c",
            NONE,
            "host-local",
        ]
    );
}

#[gpui::test]
fn grouping_is_a_projection_and_leaves_the_window_member_order_alone(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = open(cx, Grouping::Project);
    let members = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| view.read(cx).containers.active().members.clone())
    };
    let before = members(cx);
    for grouping in [Grouping::Status, Grouping::Project, Grouping::None] {
        view.update(cx, |view, cx| {
            group(view, grouping);
            cx.notify();
        });
        draw(cx);
        assert_eq!(members(cx), before, "{grouping:?}");
    }
}

#[gpui::test]
fn no_grouping_paints_no_headings_and_hosts_in_window_order(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx, Grouping::None);
    let hosts = ["host-local", "host-ssh:a", "host-ssh:b", "host-ssh:c"];
    assert_eq!(painted(cx, &hosts), hosts);
    let headings = cx.update(|_, cx| {
        view.read(cx)
            .sidebar_sections(|_| true)
            .map(|sections| sections.len())
    });
    assert_eq!(headings, None);
    assert!(cx.debug_bounds(NONE).is_none());
}

#[gpui::test]
fn switching_the_grouping_regroups_the_next_frame(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx, Grouping::None);
    assert!(cx.debug_bounds(NONE).is_none());
    view.update(cx, |view, cx| {
        group(view, Grouping::Project);
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds(NONE).is_some());
    view.update(cx, |view, cx| {
        group(view, Grouping::Status);
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds(NONE).is_none());
    assert!(cx.debug_bounds("group-status-working").is_some());
    view.update(cx, |view, cx| {
        group(view, Grouping::None);
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds("group-status-working").is_none());
}

#[gpui::test]
fn status_groups_use_a_fixed_priority(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = build(window, cx);
        // local: working (the fixture's agents). a: waiting. b: idle. c: off.
        let waiting = Arc::make_mut(view.endpoints[1].live.snapshot.as_mut().unwrap());
        waiting.agents[0].agent_status = AgentStatus::Blocked;
        let idle = Arc::make_mut(view.endpoints[2].live.snapshot.as_mut().unwrap());
        idle.agents.clear();
        view.endpoints[3].enabled = false;
        group(&mut view, Grouping::Status);
        view
    });
    cx.simulate_resize(size(px(800.), px(700.)));
    cx.run_until_parked();
    draw(cx);
    let all = [
        "group-status-stopped",
        "group-status-idle",
        "group-status-working",
        "group-status-waiting",
        "group-status-disconnected",
        "host-local",
        "host-ssh:a",
        "host-ssh:b",
        "host-ssh:c",
    ];
    let expected = [
        "group-status-waiting",
        "host-ssh:a",
        "group-status-working",
        "host-local",
        "group-status-idle",
        "host-ssh:b",
        "group-status-stopped",
        "host-ssh:c",
    ];
    assert_eq!(painted(cx, &all), expected);
    // Dropping a connection moves its session to Disconnected, below idle.
    view.update(cx, |view, cx| {
        view.endpoints[2].live.status = ConnectionStatus::Disconnected;
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds("group-status-idle").is_none());
    assert!(
        top(cx, "group-status-disconnected").unwrap() < top(cx, "group-status-stopped").unwrap()
    );
}

#[gpui::test]
fn a_heading_folds_its_group_and_the_fold_is_per_window_and_group(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx, Grouping::Project);
    assert!(cx.debug_bounds("host-ssh:a").is_some());
    let center = cx.debug_bounds(ALPHA).unwrap().center();
    cx.simulate_click(center, Modifiers::none());
    draw(cx);
    for host in ["host-ssh:a", "host-ssh:b"] {
        assert!(cx.debug_bounds(host).is_none(), "{host} is folded away");
    }
    assert!(cx.debug_bounds(ALPHA).is_some(), "the heading stays");
    assert!(cx.debug_bounds("host-ssh:c").is_some(), "other groups stay");
    // Keyboard stepping skips what is folded, too.
    let stepped: Vec<_> = cx.update(|_, cx| {
        view.read(cx)
            .sidebar_workspaces()
            .into_iter()
            .map(|(host, _)| host)
            .collect()
    });
    assert_eq!(stepped, [3, 0]);
    cx.simulate_click(center, Modifiers::none());
    draw(cx);
    assert!(cx.debug_bounds("host-ssh:a").is_some());
}

#[gpui::test]
fn the_device_filter_narrows_groups_and_drops_empty_ones(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx, Grouping::Project);
    view.update(cx, |view, cx| {
        view.device_filter = Some("ssh:a".into());
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds(ALPHA).is_some());
    assert!(cx.debug_bounds("host-ssh:a").is_some());
    for gone in ["host-ssh:b", "host-ssh:c", "host-local", NONE] {
        assert!(cx.debug_bounds(gone).is_none(), "{gone}");
    }
    // The header count is the filtered one.
    assert!(cx.debug_bounds(ALPHA_COUNT).is_some());
}

#[gpui::test]
fn search_results_stay_flat_while_grouped_and_the_groups_return_when_it_clears(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = open(cx, Grouping::Project);
    let focus = cx.update(|_, cx| view.read(cx).sidebar_search.input.read(cx).focus.clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    cx.simulate_input("herdr");
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("sidebar-search-results").is_some());
    assert!(cx.debug_bounds("spaces-scroll").is_none());
    let hits = cx.update(|_, cx| view.read(cx).sidebar_search_hits(cx).len());
    assert!(
        hits > 0,
        "the grouped Window still searches every listed host"
    );
    view.update(cx, |view, cx| {
        view.sidebar_search
            .input
            .update(cx, |input, cx| input.clear(cx));
    });
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds(NONE).is_some());
}

#[gpui::test]
fn a_workspace_drag_moves_only_within_its_host_under_grouping(cx: &mut gpui::TestAppContext) {
    // Reordering moves workspaces inside one host's list. Grouping reorders
    // hosts for display only, so a drag can never move a host across groups.
    let (view, cx) = open(cx, Grouping::Project);
    cx.update(|_, cx| {
        let view = view.read(cx);
        let order: Vec<SessionRef> = view.containers.active().members.clone();
        let hosts: Vec<_> = view
            .sidebar_sections(|_| true)
            .unwrap()
            .into_iter()
            .flat_map(|section| section.hosts)
            .collect();
        assert_eq!(hosts, [1, 2, 3, 0]);
        assert_eq!(order[0], view.endpoints[0].session_ref());
    });
}
