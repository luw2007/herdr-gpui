#![allow(clippy::unwrap_used)]

mod lifecycle;

use crate::{containers::session_ref, endpoint::Endpoint, sidebar::layout_tests::fixture_window};
use gpui::TestAppContext;
use herdr_client::ConnectTarget;

fn ssh(session: &str) -> ConnectTarget {
    ConnectTarget::Ssh {
        target: "box".into(),
        session: session.into(),
    }
}

fn saved(id: &str) -> herdr_client::SavedHost {
    herdr_client::SavedHost {
        id: id.into(),
        label: id.into(),
        target: format!("user@{id}"),
        session: "default".into(),
        enabled: true,
    }
}

fn device(view: &mut crate::HerdrWindow) {
    view.endpoints.push(Endpoint::new(
        "ssh:box".into(),
        "Box".into(),
        ssh("default"),
        true,
    ));
    view.list_all_endpoints();
}

#[gpui::test]
fn attaching_adds_a_connection_beside_the_device_and_selects_it(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        device(view);
        let default = session_ref("ssh:box", "default");
        let other = session_ref("ssh:box", "other");
        view.attach_session(default.clone(), cx);
        view.attach_session(other.clone(), cx);
        view.attach_session(other.clone(), cx);
        // Two sessions of one device, one connection each, no duplicate.
        assert_eq!(
            view.endpoints
                .iter()
                .filter(|e| e.device_id() == "ssh:box")
                .count(),
            2
        );
        let slots = &view.containers.active().members;
        assert!(slots.contains(&default) && slots.contains(&other));
        assert_eq!(view.endpoints[view.selected_endpoint].session_ref(), other);
        // The device endpoint kept its own target.
        assert!(matches!(
            &view.endpoints[1].connection.target,
            ConnectTarget::Ssh { session, .. } if session == "default"
        ));
    });
}

#[gpui::test]
fn each_logical_window_lists_only_its_own_sessions(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        device(view);
        let other = session_ref("ssh:box", "other");
        view.new_container(Some("Second"), cx).unwrap();
        assert!(view.container_is_empty());
        let local = (0..view.endpoints.len()).find(|i| view.endpoint_listed(*i));
        assert_eq!(local, None);
        view.attach_session(other.clone(), cx);
        let listed: Vec<_> = (0..view.endpoints.len())
            .filter(|i| view.endpoint_listed(*i))
            .map(|i| view.endpoints[i].session_ref())
            .collect();
        assert_eq!(listed, vec![other.clone()]);
        let first = view.containers.containers()[0].id;
        view.switch_container(first, cx);
        let listed: Vec<_> = (0..view.endpoints.len())
            .filter(|i| view.endpoint_listed(*i))
            .map(|i| view.endpoints[i].session_ref())
            .collect();
        assert!(!listed.contains(&other));
        // The same session may be attached to the first Window as well.
        view.attach_session(other.clone(), cx);
        assert_eq!(
            view.endpoints
                .iter()
                .filter(|e| e.session_ref() == other)
                .count(),
            1
        );
        assert!(
            view.containers
                .containers()
                .iter()
                .all(|c| c.members.contains(&other))
        );
    });
}

#[gpui::test]
fn removing_and_closing_are_view_only(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        device(view);
        let other = session_ref("ssh:box", "other");
        view.attach_session(other.clone(), cx);
        let index = view.endpoint_of(&other).unwrap();
        view.remove_slot(&other, cx);
        assert!(!view.containers.active().members.contains(&other));
        // Nothing was stopped or paused: the connection is untouched.
        assert_ne!(
            view.endpoints.get(index).map(|e| e.status()),
            Some("detached")
        );
        let first = view.containers.containers()[0].id;
        // A Window that lists sessions is not closable: empty it first.
        for member in view.containers.active().members.clone() {
            view.remove_slot(&member, cx);
        }
        view.close_container(first, cx).unwrap();
        assert_eq!(view.containers.containers().len(), 1);
        assert!(view.container_is_empty());
        assert!(view.endpoints.iter().all(|e| e.status() != "detached"));
    });
}

#[gpui::test]
fn stop_pauses_every_matching_connection_in_every_logical_window(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        device(view);
        let other = session_ref("ssh:box", "other");
        view.attach_session(other.clone(), cx);
        view.new_container(None, cx).unwrap();
        view.attach_session(session_ref("ssh:box", "third"), cx);
        view.pause_session_endpoints(&ssh("other"));
        let paused: Vec<_> = view
            .endpoints
            .iter()
            .filter(|e| e.status() == "detached")
            .map(|e| e.session_ref())
            .collect();
        assert_eq!(paused, vec![other]);
    });
}

#[gpui::test]
fn deleting_a_session_rewrites_every_logical_window(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        device(view);
        let gone = session_ref("ssh:box", "gone");
        view.attach_session(gone.clone(), cx);
        view.new_container(None, cx).unwrap();
        view.attach_session(gone.clone(), cx);
        view.retarget_session_for_deletion(&ssh("gone"));
        view.containers_changed(cx);
        assert!(
            view.containers
                .containers()
                .iter()
                .all(|c| !c.members.contains(&gone))
        );
        assert!(view.endpoints.iter().all(|e| e.session_ref() != gone));
        assert_eq!(
            view.endpoints
                .iter()
                .filter(|e| e.session_ref().session == "default" && e.device_id() == "ssh:box")
                .count(),
            1,
        );
    });
}

#[gpui::test]
fn a_device_leaving_the_catalog_drops_its_rows(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        view.reconcile_catalog(vec![saved("b")], Vec::new(), cx);
        view.attach_session(session_ref("ssh:b", "extra"), cx);
        view.reconcile_catalog(Vec::new(), Vec::new(), cx);
        assert!(
            view.containers
                .sessions()
                .all(|s| s.device.as_str() != "ssh:b")
        );
        assert_eq!(view.endpoints.len(), 1);
    });
}

/// A native window on the app's shared registry, as every real one is.
fn shared(cx: &mut TestAppContext) -> gpui::WindowHandle<crate::HerdrWindow> {
    cx.update(|cx| {
        if !cx.has_global::<crate::containers::Containers>() {
            cx.set_global(crate::containers::Containers::default());
        }
    });
    cx.add_window(|window, cx| {
        let mut view = fixture_window(window, cx);
        let base = view.endpoints[0].session_ref();
        view.containers = crate::containers::ContainerView::open(&base, cx);
        view
    })
}

#[gpui::test]
fn one_window_open_in_two_native_windows_follows_membership_edits(cx: &mut TestAppContext) {
    let first = shared(cx);
    let second = shared(cx);
    let shared_session = session_ref("ssh:box", "shared");
    let id = first
        .update(cx, |view, _, _| {
            device(view);
            view.containers.shown_id()
        })
        .unwrap();
    second
        .update(cx, |view, _, cx| {
            device(view);
            // Opened on the same registry: the same Window, shown on its own.
            assert_eq!(view.containers.shown_id(), id);
            view.attach_session(session_ref("ssh:box", "shared"), cx);
        })
        .unwrap();
    first
        .update(cx, |view, _, cx| {
            view.sync_containers(cx);
            assert!(view.containers.active().members.contains(&shared_session));
            // Each native window dials for itself.
            assert!(view.endpoint_of(&shared_session).is_some());
            view.remove_slot(&shared_session, cx);
        })
        .unwrap();
    second
        .update(cx, |view, _, cx| {
            view.sync_containers(cx);
            assert!(!view.containers.active().members.contains(&shared_session));
        })
        .unwrap();
}

#[gpui::test]
fn a_stale_mirror_cannot_close_a_window_another_native_window_filled(cx: &mut TestAppContext) {
    let first = shared(cx);
    let second = shared(cx);
    let extra = first
        .update(cx, |view, _, cx| {
            view.new_container(Some("Extra"), cx).unwrap()
        })
        .unwrap();
    second
        .update(cx, |view, _, cx| {
            view.sync_containers(cx);
            assert!(view.containers.set().get(extra).unwrap().members.is_empty());
        })
        .unwrap();
    // The other native window fills it; `second` does not poll before acting.
    let kept = session_ref("ssh:box", "kept");
    first
        .update(cx, |view, _, cx| {
            device(view);
            view.sync_containers(cx);
            view.edit_containers(cx, |set| set.attach(extra, kept.clone()))
                .unwrap();
        })
        .unwrap();
    second
        .update(cx, |view, _, cx| {
            assert!(view.containers.set().get(extra).unwrap().members.is_empty());
            assert_eq!(
                view.close_container(extra, cx),
                Err(crate::containers::ContainerError::NotEmpty)
            );
            assert!(
                view.containers
                    .set()
                    .get(extra)
                    .is_some_and(|c| c.members.contains(&kept))
            );
        })
        .unwrap();
}

#[gpui::test]
fn closing_a_window_moves_every_native_window_showing_it(cx: &mut TestAppContext) {
    let first = shared(cx);
    let second = shared(cx);
    let extra = first
        .update(cx, |view, _, cx| {
            view.new_container(Some("Extra"), cx).unwrap()
        })
        .unwrap();
    second
        .update(cx, |view, _, cx| {
            view.sync_containers(cx);
            view.switch_container(extra, cx);
            assert_eq!(view.containers.shown_id(), extra);
        })
        .unwrap();
    first
        .update(cx, |view, _, cx| view.close_container(extra, cx).unwrap())
        .unwrap();
    second
        .update(cx, |view, _, cx| {
            view.sync_containers(cx);
            assert_ne!(view.containers.shown_id(), extra);
            let shown = view.containers.shown_id();
            assert!(view.containers.set().get(shown).is_some());
        })
        .unwrap();
}

#[gpui::test]
fn deleting_a_session_rewrites_the_shared_window_for_every_native_window(cx: &mut TestAppContext) {
    let first = shared(cx);
    let second = shared(cx);
    let gone = session_ref("ssh:box", "gone");
    first
        .update(cx, |view, _, cx| {
            device(view);
            view.attach_session(gone.clone(), cx);
        })
        .unwrap();
    second
        .update(cx, |view, _, cx| {
            device(view);
            view.sync_containers(cx);
            assert!(view.containers.active().members.contains(&gone));
            view.retarget_session_for_deletion(&ssh("gone"));
            view.containers_changed(cx);
            // The rewrite is in the registry, not only in this window's copy.
            assert!(!view.containers.set().references(&gone));
        })
        .unwrap();
    first
        .update(cx, |view, _, cx| {
            view.sync_containers(cx);
            assert!(!view.containers.set().references(&gone));
            assert!(
                view.containers
                    .active()
                    .members
                    .contains(&session_ref("ssh:box", "default"))
            );
            assert!(view.endpoints.iter().all(|e| e.session_ref() != gone));
        })
        .unwrap();
}

#[gpui::test]
fn stopping_pauses_the_connection_in_every_native_window_and_keeps_the_member(
    cx: &mut TestAppContext,
) {
    let first = shared(cx);
    let second = shared(cx);
    let stopped = session_ref("ssh:box", "stopped");
    for handle in [first, second] {
        handle
            .update(cx, |view, _, cx| {
                device(view);
                view.sync_containers(cx);
                view.attach_session(stopped.clone(), cx);
            })
            .unwrap();
    }
    let paused = |view: &crate::HerdrWindow| {
        view.endpoints
            .iter()
            .filter(|e| e.status() == "detached")
            .map(|e| e.session_ref())
            .collect::<Vec<_>>()
    };
    first
        .update(cx, |view, window, cx| {
            view.pause_session_in_all_windows(
                &ssh("stopped"),
                window.window_handle().window_id(),
                cx,
            );
            assert_eq!(paused(view), vec![stopped.clone()]);
            // Stopping never edits the registry: the member stays listed.
            assert!(view.containers.set().references(&stopped));
        })
        .unwrap();
    second
        .update(cx, |view, _, _| {
            assert_eq!(paused(view), vec![stopped.clone()]);
            assert!(view.containers.set().references(&stopped));
        })
        .unwrap();
}

#[gpui::test]
fn one_name_on_two_devices_stays_two_members(cx: &mut TestAppContext) {
    let window = shared(cx);
    window
        .update(cx, |view, _, cx| {
            device(view);
            view.endpoints.push(Endpoint::new(
                "ssh:other".into(),
                "Other".into(),
                ssh("default"),
                true,
            ));
            view.list_all_endpoints();
            view.attach_session(session_ref("ssh:box", "same"), cx);
            view.attach_session(session_ref("ssh:other", "same"), cx);
            let members = &view.containers.active().members;
            assert!(members.contains(&session_ref("ssh:box", "same")));
            assert!(members.contains(&session_ref("ssh:other", "same")));
        })
        .unwrap();
}
