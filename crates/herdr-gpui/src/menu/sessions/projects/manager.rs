//! Project catalog and explicit saved-server folder associations.
use super::*;
use herdr_client::Binding;

impl HerdrWindow {
    /// Opens independently of session selection, including an empty catalog.
    pub(crate) fn open_project_manager(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let anchor = self.sessions_anchor.get();
        self.open_sessions(anchor, window, cx);
        if self.menu.page != Some(Page::Sessions) {
            return false;
        }
        self.sync_projects(cx);
        self.show_project_manager(window, cx);
        true
    }

    #[cfg(all(unix, test))]
    pub(crate) fn project_manager_showing(&self) -> bool {
        matches!(
            self.menu.session_edit,
            Some(Edit::Project(ProjectEdit::Manager))
        )
    }

    pub(in crate::menu) fn show_project_manager(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.error = None;
        self.menu.session_edit = Some(Edit::Project(ProjectEdit::Manager));
        window.focus(&self.menu.focus, cx);
        cx.notify();
    }

    pub(super) fn show_project_detail(
        &mut self,
        id: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.error = None;
        self.menu.session_edit = Some(Edit::Project(ProjectEdit::Detail { id }));
        window.focus(&self.menu.focus, cx);
        cx.notify();
    }

    pub(super) fn choose_project_device(
        &mut self,
        id: ProjectId,
        previous: Option<Binding>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.error = None;
        self.menu.session_edit = Some(Edit::Project(ProjectEdit::ChooseDevice { id, previous }));
        window.focus(&self.menu.focus, cx);
        cx.notify();
    }

    pub(super) fn project_device_label(&self, device: &str) -> String {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.slot.is_none() && endpoint.device_id() == device)
            .map(|endpoint| format!("{} ({device})", endpoint.label))
            .unwrap_or_else(|| format!("Saved server ({device})"))
    }

    fn manager_list(&self, selector: &'static str) -> Stateful<Div> {
        div()
            .id(selector)
            .debug_selector(move || selector.into())
            .flex()
            .flex_col()
            .gap(px(8.))
            .max_h(px(320.))
            .overflow_y_scroll()
    }

    pub(super) fn project_manager_body(&self, view: Div, cx: &mut Context<Self>) -> Div {
        let mut list = self.manager_list("project-manager-list");
        let projects = self.sessions.projects.projects();
        if projects.is_empty() {
            list = list.child(
                div().child("No projects yet. Create a project, then choose a server and folder."),
            );
        }
        for (index, project) in projects.into_iter().enumerate() {
            let id = project.id.clone();
            list = list.child(self.modal_button(
                ("project-manager-project", index),
                format!("project-manager-project-{index}"),
                project.name.clone(),
                cx,
                move |this, window, cx| this.show_project_detail(id.clone(), window, cx),
            ));
        }
        view.child(div().font_weight(FontWeight::SEMIBOLD).child("Projects"))
            .child(div().child("Manage projects and their folders on each saved server."))
            .child(list)
            .child(self.modal_button(
                "project-manager-create",
                "project-manager-create".into(),
                "Create project…",
                cx,
                |this, window, cx| this.open_project_create(None, window, cx),
            ))
    }

    pub(super) fn project_detail_body(
        &self,
        view: Div,
        id: &ProjectId,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(project) = self.sessions.projects.project(id) else {
            return view.child("Project no longer exists.");
        };
        let mut list = self.manager_list("project-bindings");
        let bindings = self.sessions.projects.bindings_of(id);
        if bindings.is_empty() {
            list = list.child(div().child("No folders bound. Add a folder on a saved server."));
        }
        for (index, binding) in bindings.into_iter().enumerate() {
            let edit = binding.clone();
            let remove = binding.clone();
            let id = id.clone();
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .min_w_0()
                    .w_full()
                    .debug_selector(move || format!("project-binding-{index}"))
                    .child(
                        div()
                            .min_w_0()
                            .whitespace_normal()
                            .child(self.project_device_label(binding.device.as_str())),
                    )
                    .child(
                        div()
                            .id(("project-binding-path", index))
                            .debug_selector(move || format!("project-binding-path-{index}"))
                            .min_w_0()
                            .w_full()
                            .overflow_x_scroll()
                            .child(binding.path.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(4.))
                            .child(self.modal_button(
                                ("project-binding-edit", index),
                                format!("project-binding-edit-{index}"),
                                "Edit…",
                                cx,
                                move |this, window, cx| {
                                    this.choose_project_device(
                                        id.clone(),
                                        Some(edit.clone()),
                                        window,
                                        cx,
                                    )
                                },
                            ))
                            .child(self.modal_button(
                                ("project-binding-remove", index),
                                format!("project-binding-remove-{index}"),
                                "Remove binding",
                                cx,
                                move |this, window, cx| {
                                    let result =
                                        Projects::update(cx, |store| store.remove_binding(&remove));
                                    match result {
                                        Ok(()) => {
                                            this.sync_projects(cx);
                                            this.show_project_detail(
                                                remove.project.clone(),
                                                window,
                                                cx,
                                            );
                                        }
                                        Err(error) => {
                                            this.menu.error = Some(error.to_string());
                                            cx.notify();
                                        }
                                    }
                                },
                            )),
                    ),
            );
        }
        let (add, rename, remove) = (id.clone(), id.clone(), id.clone());
        view.child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(project.name.clone()),
        )
        .child(div().child("Server folders (associations only; removing one never deletes files)"))
        .child(list)
        .child(self.modal_button(
            "project-binding-add",
            "project-binding-add".into(),
            "Add server folder…",
            cx,
            move |this, window, cx| this.choose_project_device(add.clone(), None, window, cx),
        ))
        .child(self.modal_button(
            "project-manager-rename",
            "project-manager-rename".into(),
            "Rename project…",
            cx,
            move |this, window, cx| this.open_project_rename(rename.clone(), window, cx),
        ))
        .child(self.modal_button(
            "project-manager-delete",
            "project-manager-delete".into(),
            "Delete project…",
            cx,
            move |this, window, cx| this.open_project_remove(remove.clone(), window, cx),
        ))
        .child(self.modal_button(
            "project-manager-back",
            "project-manager-back".into(),
            "All projects",
            cx,
            |this, window, cx| this.show_project_manager(window, cx),
        ))
    }

    pub(super) fn project_devices_body(
        &self,
        view: Div,
        id: &ProjectId,
        previous: Option<&Binding>,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut list = self.manager_list("project-device-list");
        // Base endpoints include saved offline devices; transient session slots
        // are excluded, matching the Sessions device facet's catalog.
        for (index, endpoint) in self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.slot.is_none())
            .enumerate()
        {
            let (id, device, previous) = (
                id.clone(),
                endpoint.device_id().to_owned(),
                previous.cloned(),
            );
            list = list.child(self.modal_button(
                ("project-device", index),
                format!("project-device-{index}"),
                self.project_device_label(&device),
                cx,
                move |this, window, cx| {
                    let text = previous
                        .as_ref()
                        .map_or("", |binding| binding.path.as_str());
                    let input =
                        this.text_input("Absolute folder path on this server", text, window, cx);
                    this.menu.error = None;
                    this.menu.session_edit = Some(Edit::Project(ProjectEdit::Bind {
                        id: id.clone(),
                        device: device.clone(),
                        input,
                        previous: previous.clone(),
                    }));
                    cx.notify();
                },
            ));
        }
        view.child(div().font_weight(FontWeight::SEMIBOLD).child("Choose server"))
            .child(div().child("Saved servers are available even when offline. Paths are interpreted on the selected server."))
            .child(list)
    }
}
