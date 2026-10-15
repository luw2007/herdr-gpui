#![allow(clippy::unwrap_used)]
use crate::{containers::SessionRef, endpoint::Endpoint, sidebar::layout_tests::fixture_window};
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
        let default = SessionRef::new("ssh:box", "default");
        let other = SessionRef::new("ssh:box", "other");
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
        let slots = &view.containers.active().slots;
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
        let other = SessionRef::new("ssh:box", "other");
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
        view.switch_container(0, cx);
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
                .all(|c| c.slots.contains(&other))
        );
    });
}

#[gpui::test]
fn removing_and_closing_are_view_only(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        device(view);
        let other = SessionRef::new("ssh:box", "other");
        view.attach_session(other.clone(), cx);
        let index = view.endpoint_of(&other).unwrap();
        view.remove_slot(&other, cx);
        assert!(!view.containers.active().slots.contains(&other));
        // Nothing was stopped or paused: the connection is untouched.
        assert_ne!(
            view.endpoints.get(index).map(|e| e.status()),
            Some("detached")
        );
        view.close_container(0, cx).unwrap();
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
        let other = SessionRef::new("ssh:box", "other");
        view.attach_session(other.clone(), cx);
        view.new_container(None, cx).unwrap();
        view.attach_session(SessionRef::new("ssh:box", "third"), cx);
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
        let gone = SessionRef::new("ssh:box", "gone");
        view.attach_session(gone.clone(), cx);
        view.new_container(None, cx).unwrap();
        view.attach_session(gone.clone(), cx);
        view.retarget_session_for_deletion(&ssh("gone"));
        assert!(
            view.containers
                .containers()
                .iter()
                .all(|c| !c.slots.contains(&gone))
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
        view.attach_session(SessionRef::new("ssh:b", "extra"), cx);
        view.reconcile_catalog(Vec::new(), Vec::new(), cx);
        assert!(view.containers.sessions().all(|s| s.device != "ssh:b"));
        assert_eq!(view.endpoints.len(), 1);
    });
}
