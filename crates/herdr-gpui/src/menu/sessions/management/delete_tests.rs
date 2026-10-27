use super::Row;
use crate::{containers::session_ref, projects::Projects, sidebar::layout_tests::fixture_window};
use gpui::TestAppContext;

fn assigned(cx: &mut gpui::App, device: &str, session: &str) -> bool {
    Projects::update(cx, |store| {
        let project = match store.registry().projects().first() {
            Some(project) => project.id.clone(),
            None => store.create_project("App", None).unwrap().id,
        };
        store.assign(device, session, &project).unwrap();
        true
    })
}

#[gpui::test]
fn a_deleted_unattached_session_is_forgotten_by_its_captured_ref(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let gone = session_ref("ssh:box", "old");
            let kept = session_ref("ssh:box", "keep");
            // No endpoint exists for either session.
            assert!(view.endpoint_of(&gone).is_none());
            let id = view.containers.shown_id();
            view.edit_containers(cx, |set| {
                set.attach(id, gone.clone())?;
                set.pause(gone.clone());
                set.pause(kept.clone());
                Ok(())
            })
            .unwrap();
            assigned(cx, "ssh:box", "old");
            assigned(cx, "ssh:box", "keep");
            let row = Row::Device {
                id: "ssh:box".into(),
                session: "old".into(),
            };
            assert_eq!(view.deleted_session_ref(&row), Some(gone.clone()));
            view.forget_deleted_session(&gone, cx);
            assert!(!view.containers.set().is_paused(&gone));
            assert!(view.containers.set().is_paused(&kept));
            assert!(!view.containers.references(&gone));
            assert!(
                view.containers
                    .active()
                    .members
                    .contains(&session_ref("ssh:box", "default"))
            );
            Projects::read(cx, |registry| {
                assert!(registry.assignment(&gone).is_none());
                assert!(registry.assignment(&kept).is_some());
            });
        });
    });
}
