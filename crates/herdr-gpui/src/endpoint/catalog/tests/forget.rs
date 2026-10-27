use super::*;
use crate::{projects::Projects, sidebar::layout_tests::fixture_window};
use gpui::TestAppContext;

fn bound(cx: &mut gpui::App, device: &str) {
    Projects::update(cx, |store| {
        let project = store.create_project("App", None).unwrap();
        store.bind(device, "/work/app", &project.id).unwrap();
        store.assign(device, "s1", &project.id).unwrap();
    });
}

fn holds(cx: &gpui::App, device: &str) -> bool {
    Projects::read(cx, |registry| {
        let key = herdr_client::DeviceKey::new(device).unwrap();
        let session = herdr_client::SessionRef::new(key, "s1");
        registry.assignment(&session).is_some()
    })
}

#[gpui::test]
fn only_a_device_the_catalog_dropped_loses_its_project_links(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            Projects::default().install(cx);
            view.reconcile_catalog(vec![host("a", true)], Vec::new(), cx);
            bound(cx, "ssh:a");
            // Disabled: still listed, links stay.
            view.reconcile_catalog(vec![host("a", false)], Vec::new(), cx);
            assert!(holds(cx, "ssh:a"));
            // Re-enabled, then removed from the catalog for good.
            view.reconcile_catalog(vec![host("a", true)], Vec::new(), cx);
            assert!(holds(cx, "ssh:a"));
            view.reconcile_catalog(Vec::new(), Vec::new(), cx);
            assert!(!holds(cx, "ssh:a"));
        });
    });
}
