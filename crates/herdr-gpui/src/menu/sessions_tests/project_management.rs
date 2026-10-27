//! Project management is reachable without sessions or a live server. Every
//! mutation below goes through the painted controls and their focused input.
use super::*;
use crate::projects::Projects;
use herdr_client::Binding;

fn bindings(cx: &mut VisualTestContext) -> Vec<Binding> {
    cx.update(|_, cx| {
        Projects::read(cx, |registry| {
            registry
                .projects()
                .into_iter()
                .flat_map(|project| registry.bindings_of(&project.id))
                .collect()
        })
    })
}

fn submit(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_input(text);
    draw(cx);
    click(cx, "session-edit-submit");
    cx.run_until_parked();
    draw(cx);
}

fn create_from_empty(cx: &mut VisualTestContext) {
    let toolbar = bounds(cx, "sessions-toolbar");
    let manage = bounds(cx, "sessions-projects-manage");
    assert!(manage.top() >= toolbar.top() && manage.bottom() <= toolbar.bottom());
    click(cx, "sessions-projects-manage");
    draw(cx);
    click(cx, "project-manager-create");
    draw(cx);
    submit(cx, "Atlas");
    // No session was selected: creation must continue straight to the chooser.
    assert!(cx.debug_bounds("project-device-0").is_some());
    assert!(bindings(cx).is_empty());
}

#[gpui::test]
fn empty_sessions_can_bind_an_offline_saved_server_edit_and_unbind(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, Vec::new(), development("default"));
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            // This saved endpoint has no scan answer and no live connection.
            view.endpoints
                .push(remote("ssh:build", "Build server", "build.invalid", "main"));
        });
    });
    draw(cx);
    create_from_empty(cx);
    assert!(cx.debug_bounds("project-device-1").is_some());
    click(cx, "project-device-1");
    draw(cx);
    let path = "/srv/team/atlas/worktrees/a-folder-with-a-long-unabridged-name";
    submit(cx, path);
    assert!(cx.debug_bounds("project-binding-0").is_some());
    let original = bindings(cx);
    assert_eq!(original.len(), 1);
    assert_eq!(original[0].device.as_str(), "ssh:build");
    assert_eq!(original[0].path, path);
    cx.update(|_, cx| assert_eq!(view.read(cx).selected_endpoint, 0));

    click(cx, "project-binding-edit-0");
    draw(cx);
    click(cx, "project-device-1");
    draw(cx);
    // The previous path starts selected, just as it does for a user's edit.
    submit(cx, "relative/path");
    assert_eq!(
        bindings(cx),
        original,
        "invalid edits preserve the association"
    );
    assert!(cx.debug_bounds("session-edit-submit").is_some());
    cx.update(|_, cx| assert!(view.read(cx).menu.error.is_some()));

    // Correct the focused form without dismissing it, then remove the binding.
    cx.simulate_keystrokes("cmd-a");
    submit(cx, path);
    assert!(cx.debug_bounds("project-binding-0").is_some());
    click(cx, "project-binding-remove-0");
    cx.run_until_parked();
    draw(cx);
    assert!(bindings(cx).is_empty());
    assert!(cx.debug_bounds("project-binding-0").is_none());
    click(cx, "project-manager-back");
    draw(cx);
    assert!(cx.debug_bounds("project-manager-project-0").is_some());
    click(cx, "project-manager-project-0");
    draw(cx);
    assert!(cx.debug_bounds("project-binding-add").is_some());
}

#[gpui::test]
fn a_project_can_keep_distinct_local_and_remote_bindings(cx: &mut TestAppContext) {
    let (view, cx) = open_list(cx, Vec::new(), development("default"));
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.endpoints
                .push(remote("ssh:build", "Build server", "build.invalid", "main"));
        });
    });
    draw(cx);
    create_from_empty(cx);
    click(cx, "project-device-0");
    draw(cx);
    submit(cx, "/code/atlas");
    let local = bindings(cx).pop().unwrap();
    let local_device = cx.update(|_, cx| view.read(cx).endpoints[0].id.clone());
    assert_eq!(local.device.as_str(), local_device);

    click(cx, "project-binding-add");
    draw(cx);
    click(cx, "project-device-1");
    draw(cx);
    submit(cx, "/code/atlas");
    let all = bindings(cx);
    assert_eq!(all.len(), 2);
    assert!(all.contains(&local));
    assert!(all.iter().any(|binding| {
        binding.device.as_str() == "ssh:build"
            && binding.path == local.path
            && binding.project == local.project
    }));
    assert!(cx.debug_bounds("project-binding-0").is_some());
    assert!(cx.debug_bounds("project-binding-1").is_some());
}
