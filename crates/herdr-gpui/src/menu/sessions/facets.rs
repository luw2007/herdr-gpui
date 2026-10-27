//! The Device and Project filters of the sessions manager: two buttons under
//! the search field, each opening a wrapped row of choices. They combine with
//! the run-state filter and the search text.
use super::catalog::ProjectPick;
use crate::HerdrWindow;
use gpui::{prelude::*, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::menu) enum Facet {
    Device,
    Project,
}

impl Facet {
    const EACH: [Self; 2] = [Self::Device, Self::Project];

    fn name(self) -> &'static str {
        match self {
            Self::Device => "Device",
            Self::Project => "Project",
        }
    }
}

/// One selectable value of a facet.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    AnyDevice,
    Device(String),
    AnyProject,
    NoProject,
    Project(herdr_client::ProjectId),
}

impl HerdrWindow {
    /// Every value `facet` can take, with its label, in paint order. Devices
    /// come from the device catalog, projects from the global registry.
    fn facet_choices(&self, facet: Facet) -> Vec<(Choice, String)> {
        match facet {
            Facet::Device => {
                let mut choices = vec![(Choice::AnyDevice, "All devices".to_owned())];
                choices.extend(
                    self.endpoints
                        .iter()
                        .enumerate()
                        .filter(|(_, endpoint)| endpoint.slot.is_none())
                        .map(|(index, endpoint)| {
                            let label = if index == 0 {
                                format!("{} (this machine)", endpoint.label)
                            } else {
                                endpoint.label.clone()
                            };
                            (Choice::Device(endpoint.id.clone()), label)
                        }),
                );
                choices
            }
            Facet::Project => {
                let mut choices = vec![
                    (Choice::AnyProject, "All projects".to_owned()),
                    (Choice::NoProject, "No project".to_owned()),
                ];
                choices.extend(
                    self.sessions
                        .projects
                        .projects()
                        .into_iter()
                        .map(|project| (Choice::Project(project.id.clone()), project.name.clone())),
                );
                choices
            }
        }
    }

    fn facet_current(&self, facet: Facet) -> Choice {
        let scope = self.session_scope();
        match facet {
            Facet::Device => scope
                .facets
                .device
                .map_or(Choice::AnyDevice, Choice::Device),
            Facet::Project => match scope.facets.project {
                ProjectPick::Any => Choice::AnyProject,
                ProjectPick::Unassigned => Choice::NoProject,
                ProjectPick::Is(id) => Choice::Project(id),
            },
        }
    }

    /// The label of the current choice; a value that left the catalog reads
    /// as itself rather than as "All".
    fn facet_summary(&self, facet: Facet) -> String {
        let current = self.facet_current(facet);
        self.facet_choices(facet)
            .into_iter()
            .find(|(choice, _)| *choice == current)
            .map_or_else(
                || match current {
                    Choice::Device(id) => id,
                    _ => "Unknown project".to_owned(),
                },
                |(_, label)| label,
            )
    }

    fn pick_facet(&mut self, choice: Choice, cx: &mut Context<Self>) {
        self.change_scope(
            |scope| match choice {
                Choice::AnyDevice => scope.facets.device = None,
                Choice::Device(id) => scope.facets.device = Some(id),
                Choice::AnyProject => scope.facets.project = ProjectPick::Any,
                Choice::NoProject => scope.facets.project = ProjectPick::Unassigned,
                Choice::Project(id) => scope.facets.project = ProjectPick::Is(id),
            },
            cx,
        );
        if let Some(view) = &mut self.menu.sessions_view {
            view.facet_open = None;
        }
        cx.notify();
    }

    fn facet_height_budget(&self) -> Pixels {
        if self.sessions_expanded() {
            crate::menu::devices::list_height(self.menu.anchor.y)
        } else {
            // Chrome constrains compact pickers to the side with available
            // room; a top strip anchor must not collapse their choice lists.
            px(420.)
        }
    }

    /// The height the open facet's choices may take before they scroll.
    pub(in crate::menu) fn facet_room(&self) -> Pixels {
        (self.facet_height_budget() / 4.).max(px(self.config.ui.line_height() + 12.))
    }

    /// The two facet buttons and, under them, the open facet's choices.
    pub(super) fn render_session_facets(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let theme = &self.theme;
        let open = self
            .menu
            .sessions_view
            .as_ref()
            .and_then(|view| view.facet_open);
        let mut buttons = div().flex().flex_wrap().gap(px(4.)).items_center();
        for facet in Facet::EACH {
            let active = self.facet_current(facet)
                != match facet {
                    Facet::Device => Choice::AnyDevice,
                    Facet::Project => Choice::AnyProject,
                };
            let summary = self.facet_summary(facet);
            buttons = buttons.child(
                div()
                    .id(("session-facet", facet as usize))
                    .debug_selector(move || format!("sessions-facet-{}", facet.name()))
                    .max_w_full()
                    .min_w_0()
                    .truncate()
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(crate::config::corners::CONTROL))
                    .border_1()
                    .border_color(rgb(theme.active))
                    .cursor_pointer()
                    .when(open == Some(facet) || active, |s| s.bg(rgb(theme.active)))
                    .text_color(rgb(if active || open == Some(facet) {
                        theme.foreground
                    } else {
                        theme.muted
                    }))
                    .child(format!("{}: {summary} \u{25be}", facet.name()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(view) = &mut this.menu.sessions_view {
                            view.facet_open = (view.facet_open != Some(facet)).then_some(facet);
                        }
                        this.focus_session_search(window, cx);
                        cx.notify();
                    })),
            );
        }
        buttons = buttons.child(
            div()
                .id("sessions-projects-manage")
                .debug_selector(|| "sessions-projects-manage".into())
                .px(px(8.))
                .py(px(2.))
                .rounded(px(crate::config::corners::CONTROL))
                .border_1()
                .border_color(rgb(theme.active))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(theme.active)))
                .child("Manage projects…")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.show_project_manager(window, cx);
                })),
        );
        let mut rows = vec![buttons];
        if let Some(facet) = open {
            let current = self.facet_current(facet);
            // The choices scroll inside a share of the list's own height, so
            // hundreds of projects never push the sessions below them out of
            // the panel and every choice stays reachable.
            let room =
                (self.facet_height_budget() / 3.).max(px(self.config.ui.line_height() * 2. + 16.));
            let mut choices = div()
                .id(("session-facet-scroll", facet as usize))
                .debug_selector(|| "sessions-facet-choices".into())
                .flex()
                .flex_wrap()
                .content_start()
                .gap(px(4.))
                .items_center()
                .flex_1()
                .min_h_0()
                .max_h(room)
                .overflow_y_scroll();
            for (index, (choice, label)) in self.facet_choices(facet).into_iter().enumerate() {
                let selected = choice == current;
                choices = choices.child(
                    div()
                        .id(("session-facet-choice", index))
                        .debug_selector(move || format!("sessions-choice-{}-{index}", facet.name()))
                        .max_w_full()
                        .min_w_0()
                        .truncate()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(px(crate::config::corners::CONTROL))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(theme.active)))
                        .when(selected, |s| s.bg(rgb(theme.active)))
                        .text_color(rgb(if selected {
                            theme.foreground
                        } else {
                            theme.muted
                        }))
                        .child(label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick_facet(choice.clone(), cx);
                            this.focus_session_search(window, cx);
                        })),
                );
            }
            // Give flex a bounded basis, not the intrinsic height of hundreds
            // of wrapped choices; keep at least one choice row reachable.
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .h(room)
                    .min_h(px(self.config.ui.line_height() + 4.))
                    .child(choices),
            );
        }
        rows
    }
}
