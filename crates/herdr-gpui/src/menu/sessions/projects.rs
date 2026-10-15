//! Projects in the sessions manager: which project a row resolves to, and the
//! small modal that moves a session between projects, makes, renames, binds
//! and removes them. A session is always named by (device endpoint id, session
//! name); a project change never touches the session itself.
use super::*;
use crate::{projects::Projects, search_input::SearchInput};
use herdr_client::{ProjectId, RepoKey, Source};

/// The modal's state. Forms capture identities, never row ordinals.
pub(in crate::menu) enum ProjectEdit {
    /// Pick the project of `row`.
    Move {
        row: Row,
    },
    /// Name a new project, then put `row` in it when there is one.
    Create {
        row: Option<Row>,
        input: Entity<SearchInput>,
        /// The session's repository, offered as the project's identity.
        repo: Option<RepoKey>,
        use_repo: bool,
    },
    Rename {
        id: ProjectId,
        input: Entity<SearchInput>,
    },
    /// Bind a folder on `device` to the project.
    Bind {
        id: ProjectId,
        device: String,
        input: Entity<SearchInput>,
    },
    Remove {
        id: ProjectId,
    },
}

impl ProjectEdit {
    pub(in crate::menu) fn input(&self) -> Option<&Entity<SearchInput>> {
        match self {
            Self::Create { input, .. } | Self::Rename { input, .. } | Self::Bind { input, .. } => {
                Some(input)
            }
            Self::Move { .. } | Self::Remove { .. } => None,
        }
    }
}

/// The session a row names: its device's endpoint id and its name.
pub(super) fn identity(device: &str, row: &Row) -> Option<(String, String)> {
    match row {
        Row::Local(name) => Some((device.to_owned(), name.clone())),
        Row::Device { id, session } => Some((id.clone(), session.clone())),
        Row::Add(_) => None,
    }
}

/// How a resolution is told on a row: subtle, and never part of its identity.
pub(super) fn source_note(source: &Source) -> &'static str {
    match source {
        Source::Assigned => "assigned",
        Source::Path(_) => "by folder",
        Source::Repository(_) => "by repository",
    }
}

impl HerdrWindow {
    /// Take the store's registry. Called when the store changes and when the
    /// manager opens, since a window may have missed earlier changes.
    pub(crate) fn sync_projects(&mut self, cx: &mut Context<Self>) {
        self.sessions.projects = Projects::read(cx, Clone::clone);
        self.sync_session_selection();
    }

    /// The project name and deciding source of a session, if any.
    pub(super) fn session_project(
        &self,
        device: &str,
        session: &str,
        workspaces: &[SessionWorkspace],
    ) -> Option<(String, Source)> {
        resolved(&self.sessions.projects, device, session, workspaces)
    }

    pub(super) fn local_project(&self, session: &LocalSession) -> Option<(String, Source)> {
        self.session_project(&self.endpoints[0].id, &session.name, &session.workspaces)
    }

    /// `detail` followed by the project's source, e.g. `running · by folder`.
    pub(super) fn with_source(
        &self,
        detail: String,
        device: &str,
        session: &str,
        workspaces: &[SessionWorkspace],
    ) -> String {
        match self.session_project(device, session, workspaces) {
            Some((_, source)) => format!("{detail} · {}", source_note(&source)),
            None => detail,
        }
    }

    /// The saved workspaces of the session a row names, as last scanned.
    fn row_workspaces(&self, row: &Row) -> Vec<SessionWorkspace> {
        identity(&self.endpoints[0].id, row)
            .and_then(|(device, session)| self.saved_workspaces(&device, &session))
            .unwrap_or_default()
    }

    /// The saved workspaces of a session on the device with endpoint id
    /// `device`, as the manager last scanned them; `None` when no scan has
    /// seen the session. Never starts a scan.
    pub(crate) fn saved_workspaces(
        &self,
        device: &str,
        session: &str,
    ) -> Option<Vec<SessionWorkspace>> {
        if device == self.endpoints[0].id {
            return self
                .sessions
                .entries
                .iter()
                .find(|found| found.name == session)
                .map(|found| found.workspaces.clone());
        }
        match self.sessions.devices.answers.get(device) {
            Some(DeviceScan::Sessions(sessions)) => sessions
                .iter()
                .find(|found| found.name == session)
                .map(|found| found.workspaces.clone()),
            _ => None,
        }
    }

    /// Open the Sessions manager on its delete-project confirmation, for
    /// `herdr-gpui project delete`. The user confirms or cancels there;
    /// nothing is deleted here. Whether the dialog is showing.
    #[cfg(unix)]
    pub(crate) fn confirm_project_removal(
        &mut self,
        id: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let anchor = self.sessions_anchor.get();
        self.open_sessions(anchor, window, cx);
        if self.menu.page != Some(Page::Sessions) || self.sessions.projects.project(&id).is_none() {
            return false;
        }
        self.open_project_remove(id, window, cx);
        true
    }

    fn row_project_id(&self, row: &Row) -> Option<ProjectId> {
        let (device, session) = identity(&self.endpoints[0].id, row)?;
        let workspaces = self.row_workspaces(row);
        let key = herdr_client::DeviceKey::new(device).ok()?;
        self.sessions
            .projects
            .resolve(&herdr_client::SessionRef::new(key, session), &workspaces)
            .project
    }

    pub(in crate::menu) fn open_project_move(
        &mut self,
        row: Row,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if row.is_session() {
            self.menu.error = None;
            self.menu.session_edit = Some(Edit::Project(ProjectEdit::Move { row }));
            window.focus(&self.menu.focus, cx);
            cx.notify();
        }
    }

    /// The project whose delete confirmation is showing.
    #[cfg(test)]
    pub(crate) fn project_removal_pending(&self) -> Option<ProjectId> {
        match &self.menu.session_edit {
            Some(Edit::Project(ProjectEdit::Remove { id })) => Some(id.clone()),
            _ => None,
        }
    }

    /// The user's Enter on the open project form.
    #[cfg(test)]
    pub(crate) fn confirm_project_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit_project_edit(window, cx);
    }

    fn text_input(
        &self,
        placeholder: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SearchInput> {
        let input = cx.new(|cx| {
            let mut input = SearchInput::new(cx);
            input.set_placeholder(placeholder, cx);
            input.set_appearance(self.config.ui.clone(), self.theme.clone(), cx);
            input.set_text_selected(text, cx);
            input
        });
        window.focus(&input.read(cx).focus.clone(), cx);
        input
    }

    fn open_project_create(
        &mut self,
        row: Option<Row>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repo = row
            .as_ref()
            .and_then(|row| self.row_workspaces(row).into_iter().find_map(|w| w.repo));
        let input = self.text_input("Project name", "", window, cx);
        let use_repo = repo.is_some();
        self.menu.error = None;
        self.menu.session_edit = Some(Edit::Project(ProjectEdit::Create {
            row,
            input,
            repo,
            use_repo,
        }));
        cx.notify();
    }

    fn open_project_rename(&mut self, id: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .sessions
            .projects
            .project(&id)
            .map(|project| project.name.clone())
            .unwrap_or_default();
        let input = self.text_input("Project name", &name, window, cx);
        self.menu.error = None;
        self.menu.session_edit = Some(Edit::Project(ProjectEdit::Rename { id, input }));
        cx.notify();
    }

    fn open_project_bind(
        &mut self,
        id: ProjectId,
        device: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.text_input("Absolute folder path on this device", "", window, cx);
        self.menu.error = None;
        self.menu.session_edit = Some(Edit::Project(ProjectEdit::Bind { id, device, input }));
        cx.notify();
    }

    fn open_project_remove(&mut self, id: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        self.menu.error = None;
        self.menu.session_edit = Some(Edit::Project(ProjectEdit::Remove { id }));
        window.focus(&self.menu.focus, cx);
        cx.notify();
    }

    /// Run a store change; on failure keep the modal open and say why.
    fn project_change(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Projects) -> crate::Result<()>,
    ) {
        match Projects::update(cx, change) {
            Ok(()) => {
                self.sync_projects(cx);
                self.cancel_session_edit(window, cx);
            }
            Err(error) => {
                self.menu.error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn assign_row(
        &mut self,
        row: &Row,
        project: Option<ProjectId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((device, session)) = identity(&self.endpoints[0].id, row) else {
            return;
        };
        self.project_change(window, cx, |store| match project {
            Some(id) => store.assign(&device, &session, &id),
            None => store.unassign(&device, &session).map(|_| ()),
        });
    }

    /// Submit the open project form (Enter or its button).
    pub(super) fn submit_project_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Edit::Project(edit)) = &self.menu.session_edit else {
            return;
        };
        if edit
            .input()
            .is_some_and(|input| input.read(cx).is_composing())
        {
            return;
        }
        let text = edit.input().map(|input| input.read(cx).text().to_owned());
        match edit {
            ProjectEdit::Move { .. } => {}
            ProjectEdit::Create {
                row,
                repo,
                use_repo,
                ..
            } => {
                let (row, repo) = (row.clone(), repo.clone().filter(|_| *use_repo));
                let name = text.unwrap_or_default();
                let device = self.endpoints[0].id.clone();
                self.project_change(window, cx, |store| {
                    let project = store.create_project(&name, repo)?;
                    if let Some((device, session)) =
                        row.as_ref().and_then(|row| identity(&device, row))
                    {
                        store.assign(&device, &session, &project.id)?;
                    }
                    Ok(())
                });
            }
            ProjectEdit::Rename { id, .. } => {
                let (id, name) = (id.clone(), text.unwrap_or_default());
                self.project_change(window, cx, |store| store.rename_project(&id, &name));
            }
            ProjectEdit::Bind { id, device, .. } => {
                let (id, device, path) = (id.clone(), device.clone(), text.unwrap_or_default());
                self.project_change(window, cx, |store| {
                    store.bind(&device, &path, &id).map(|_| ())
                });
            }
            ProjectEdit::Remove { id } => {
                let id = id.clone();
                self.project_change(window, cx, |store| store.remove_project(&id).map(|_| ()));
            }
        }
    }

    fn modal_button(
        &self,
        id: impl Into<ElementId>,
        selector: String,
        label: impl Into<SharedString>,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        div()
            .id(id)
            .debug_selector(move || selector.clone())
            .px(px(12.))
            .py(px(6.))
            .min_w_0()
            .truncate()
            .rounded(px(crate::config::corners::CONTROL))
            .border_1()
            .border_color(rgb(theme.active))
            .hover(|s| s.bg(rgb(theme.active)))
            .cursor_pointer()
            .child(label.into())
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
    }

    /// The modal body for a project edit, without the shared Cancel/submit bar.
    pub(super) fn project_modal_body(
        &self,
        view: Div,
        edit: &ProjectEdit,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let title = |text: &str| {
            div()
                .text_size(px(self.config.ui.size * 1.35))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text.to_owned())
        };
        let note = |text: &str| {
            div()
                .text_color(rgb(theme.subtext()))
                .child(text.to_owned())
        };
        match edit {
            ProjectEdit::Move { row } => {
                let current = self.row_project_id(row);
                let mut list = div().flex().flex_col().gap(px(4.));
                for (index, project) in self.sessions.projects.projects().into_iter().enumerate() {
                    let (id, row) = (project.id.clone(), row.clone());
                    let mark = if current.as_ref() == Some(&id) {
                        "✓ "
                    } else {
                        ""
                    };
                    list = list.child(self.modal_button(
                        ("project-choice", index),
                        format!("project-choice-{index}"),
                        format!("{mark}{}", project.name),
                        cx,
                        move |this, window, cx| this.assign_row(&row, Some(id.clone()), window, cx),
                    ));
                }
                let new_row = row.clone();
                let none_row = row.clone();
                list = list
                    .child(self.modal_button(
                        "project-new",
                        "project-new".into(),
                        "New project…",
                        cx,
                        move |this, window, cx| {
                            this.open_project_create(Some(new_row.clone()), window, cx)
                        },
                    ))
                    .child(self.modal_button(
                        "project-none",
                        "project-none".into(),
                        "No project",
                        cx,
                        move |this, window, cx| this.assign_row(&none_row, None, window, cx),
                    ));
                let mut view = view
                    .child(title("Move to project"))
                    .child(note("Only this app remembers projects; the session, its folders and worktrees are not touched."))
                    .child(list);
                if let Some(id) = current {
                    let device = identity(&self.endpoints[0].id, row)
                        .map(|(device, _)| device)
                        .unwrap_or_default();
                    let (rename, bind, remove) = (id.clone(), id.clone(), id);
                    view = view.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(self.modal_button(
                                "project-rename",
                                "project-rename".into(),
                                "Rename…",
                                cx,
                                move |this, window, cx| {
                                    this.open_project_rename(rename.clone(), window, cx)
                                },
                            ))
                            .child(self.modal_button(
                                "project-bind",
                                "project-bind".into(),
                                "Bind this device folder…",
                                cx,
                                move |this, window, cx| {
                                    this.open_project_bind(bind.clone(), device.clone(), window, cx)
                                },
                            ))
                            .child(self.modal_button(
                                "project-remove",
                                "project-remove".into(),
                                "Delete project…",
                                cx,
                                move |this, window, cx| {
                                    this.open_project_remove(remove.clone(), window, cx)
                                },
                            )),
                    );
                }
                view
            }
            ProjectEdit::Create {
                input,
                repo,
                use_repo,
                ..
            } => {
                let mut view = view.child(title("New project")).child(input.clone());
                if let Some(repo) = repo {
                    let (git, task) = (*use_repo, !*use_repo);
                    view = view
                        .child(self.modal_button(
                            "project-kind-git",
                            "project-kind-git".into(),
                            format!("{}Git repository {repo}", if git { "● " } else { "○ " }),
                            cx,
                            |this, _, cx| this.set_project_use_repo(true, cx),
                        ))
                        .child(self.modal_button(
                            "project-kind-task",
                            "project-kind-task".into(),
                            format!(
                                "{}Task project (no repository)",
                                if task { "● " } else { "○ " }
                            ),
                            cx,
                            |this, _, cx| this.set_project_use_repo(false, cx),
                        ));
                }
                view.child(note("A git project joins every device's clone automatically. A task project needs a folder bound on each device."))
            }
            ProjectEdit::Rename { input, .. } => {
                view.child(title("Rename project")).child(input.clone())
            }
            ProjectEdit::Bind { input, .. } => view
                .child(title("Bind this device folder"))
                .child(input.clone())
                .child(note(
                    "Sessions whose workspace is in this folder join the project on this device.",
                )),
            ProjectEdit::Remove { id } => {
                let name = self
                    .sessions
                    .projects
                    .project(id)
                    .map_or("", |p| p.name.as_str());
                view.child(title("Delete project?"))
                    .child(div().min_w_0().truncate().child(name.to_owned()))
                    .child(note("Sessions are not stopped and no folder or worktree is deleted. Sessions become unassigned, or are inferred again from their repository."))
            }
        }
    }

    fn set_project_use_repo(&mut self, value: bool, cx: &mut Context<Self>) {
        if let Some(Edit::Project(ProjectEdit::Create { use_repo, repo, .. })) =
            &mut self.menu.session_edit
        {
            *use_repo = value && repo.is_some();
            cx.notify();
        }
    }

    /// The submit button's label for a project form, `None` for the picker.
    pub(super) fn project_submit_label(edit: &ProjectEdit) -> Option<&'static str> {
        match edit {
            ProjectEdit::Move { .. } => None,
            ProjectEdit::Create { .. } => Some("Create project"),
            ProjectEdit::Rename { .. } => Some("Rename"),
            ProjectEdit::Bind { .. } => Some("Bind folder"),
            ProjectEdit::Remove { .. } => Some("Delete project"),
        }
    }
}

/// The project name and deciding source for a session; pure so it is testable
/// without a window.
pub(super) fn resolved(
    registry: &herdr_client::Registry,
    device: &str,
    session: &str,
    workspaces: &[SessionWorkspace],
) -> Option<(String, Source)> {
    let key = herdr_client::DeviceKey::new(device).ok()?;
    let resolution = registry.resolve(&herdr_client::SessionRef::new(key, session), workspaces);
    let project = registry.project(&resolution.project?)?;
    Some((project.name.clone(), resolution.source?))
}
