//! Lifecycle regressions of the session connections a Window keeps: limits,
//! stop, delete and device edits must leave connections, the registry and the
//! screen in agreement.
use super::*;
use crate::containers::{Containers, MAX_SLOTS};

#[gpui::test]
fn a_refused_attach_leaves_no_connection_behind(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        device(view);
        let shown = view.containers.shown_id();
        view.edit_containers(cx, |set| {
            for index in 0..MAX_SLOTS {
                // Fills the Window; the attaches past its limit fail.
                let _ = set.attach(shown, session_ref("ssh:box", format!("full{index}")));
            }
            Ok(())
        })
        .unwrap();
        let before = view.endpoints.len();
        let overflow = session_ref("ssh:box", "overflow");
        view.attach_session(overflow.clone(), cx);
        assert!(view.local_error.is_some());
        assert_eq!(view.endpoints.len(), before);
        assert!(view.endpoint_of(&overflow).is_none());
    });
}

#[gpui::test]
fn a_stop_survives_a_restart_until_the_session_is_chosen_again(cx: &mut TestAppContext) {
    let first = shared(cx);
    let stopped = session_ref("ssh:box", "stopped");
    first
        .update(cx, |view, window, cx| {
            device(view);
            view.attach_session(stopped.clone(), cx);
            view.pause_session_in_all_windows(
                &ssh("stopped"),
                window.window_handle().window_id(),
                cx,
            );
            assert!(Containers::read(cx, |set| set.is_paused(&stopped)));
        })
        .unwrap();
    // A native window opened after the stop stands for the next launch: it
    // builds its connections from the registry alone.
    let relaunched = shared(cx);
    relaunched
        .update(cx, |view, _, cx| {
            device(view);
            view.materialize_slots();
            let index = view.endpoint_of(&stopped).unwrap();
            assert_eq!(view.endpoints[index].status(), "detached");
            // Polling must not dial it.
            view.poll_endpoints(cx);
            assert!(view.endpoints[index].connection.handle.is_none());
            assert_eq!(view.endpoints[index].status(), "detached");
            let id = view.endpoints[index].id.clone();
            view.select_listed(&id, cx);
            assert!(!Containers::read(cx, |set| set.is_paused(&stopped)));
            assert_ne!(view.endpoints[index].status(), "detached");
        })
        .unwrap();
}

#[gpui::test]
fn a_recreated_session_is_not_confused_with_the_deleted_ones_default(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        // The device was saved on `base`, so it owns no `default` connection.
        view.endpoints.push(Endpoint::new(
            "ssh:box".into(),
            "Box".into(),
            ssh("base"),
            true,
        ));
        view.list_all_endpoints();
        let gone = session_ref("ssh:box", "gone");
        view.attach_session(gone.clone(), cx);
        view.retarget_session_for_deletion(&ssh("gone"));
        view.containers_changed(cx);
        assert!(view.endpoint_of(&gone).is_none());
        view.attach_session(gone.clone(), cx);
        assert!(view.endpoint_ids_unique());
        assert_eq!(view.endpoints[view.selected_endpoint].session_ref(), gone);
        let id = view.endpoints[view.selected_endpoint].id.clone();
        assert!(view.switch_endpoint(&id, cx));
        assert_eq!(view.endpoints[view.selected_endpoint].session_ref(), gone);
    });
}

#[gpui::test]
fn the_deleting_window_publishes_the_replacement_at_once(cx: &mut TestAppContext) {
    let window = shared(cx);
    let gone = session_ref("ssh:box", "gone");
    window
        .update(cx, |view, window, cx| {
            device(view);
            view.attach_session(gone.clone(), cx);
            assert!(Containers::read(cx, |set| set.references(&gone)));
            view.retire_session_for_deletion(&ssh("gone"), window.window_handle().window_id(), cx);
            // No layout save or poll ran in between.
            assert!(!Containers::read(cx, |set| set.references(&gone)));
            assert!(Containers::read(cx, |set| {
                set.references(&session_ref("ssh:box", "default"))
            }));
        })
        .unwrap();
}

#[gpui::test]
fn editing_the_shown_device_keeps_screen_and_selected_member_together(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    view.update(cx, |view, cx| {
        view.reconcile_catalog(vec![saved("b")], Vec::new(), cx);
        let extra = session_ref("ssh:b", "extra");
        view.attach_session(extra.clone(), cx);
        assert_eq!(view.endpoints[view.selected_endpoint].session_ref(), extra);
        let mut edited = saved("b");
        edited.target = "user@elsewhere".into();
        view.reconcile_catalog(vec![edited], Vec::new(), cx);
        assert_eq!(view.containers.active().selected.as_ref(), Some(&extra));
        assert_eq!(view.endpoints[view.selected_endpoint].session_ref(), extra);
        assert!(view.endpoint_ids_unique());
        // Disabling leaves nothing to show for it; the choice is only parked
        // until the device returns.
        let mut disabled = saved("b");
        disabled.enabled = false;
        view.reconcile_catalog(vec![disabled], Vec::new(), cx);
        assert_eq!(view.containers.active().selected.as_ref(), Some(&extra));
        assert!(view.container_restore_pending);
        assert_ne!(view.endpoints[view.selected_endpoint].session_ref(), extra);
    });
}
