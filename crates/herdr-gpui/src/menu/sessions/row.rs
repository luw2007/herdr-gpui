//! One painted session row: label, state dot and the project, stop and delete
//! actions.
use super::{Row, Spec, colors};
use crate::{
    HerdrWindow,
    containers::{Grouping, session_ref},
    session_mr::SessionMr,
};
use gpui::{prelude::*, *};
use herdr_client::ConnectTarget;

impl HerdrWindow {
    pub(super) fn sessions_row(
        &self,
        row: usize,
        choice: Row,
        selected: bool,
        spec: &Spec,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &self.theme;
        let font = &self.config.ui;
        let matches_target = |target: &ConnectTarget| match (&choice, target) {
            (Row::Local(name), ConnectTarget::Session { name: deleted, .. }) => name == deleted,
            (Row::Device { id, session }, target) if target.is_remote() => {
                target.remote_session() == Some(session.as_str())
                    && self.endpoints.iter().any(|endpoint| {
                        endpoint.id == *id
                            && endpoint.connection.target.remote_host() == target.remote_host()
                    })
            }
            _ => false,
        };
        let deleting = self
            .sessions
            .mutation_target
            .as_ref()
            .is_some_and(&matches_target);
        let remaining = self
            .sessions
            .departure
            .as_ref()
            .filter(|departure| matches_target(&departure.target))
            .map(|departure| departure.remaining(std::time::Instant::now()));
        let badge = self.sessions_row_badge(&choice);
        let lines = 2. + f32::from(spec.context.is_some()) + f32::from(badge.is_some());
        let height = font.line_height() * lines + 16.;
        let removal = choice.clone();
        let removal_target = self.management_target(&choice);
        let removable = !matches!(choice, Row::Add(_));
        let hover_row = choice.clone();
        let stop_row = choice.clone();
        let move_row = choice.clone();
        let stop_target = self.management_target(&choice);
        let deletion_color = if self.session_delete_reason(&choice).is_none() {
            colors::danger(theme)
        } else if selected || spec.checked {
            rgb(theme.foreground)
        } else {
            rgb(theme.muted)
        };
        let view = div()
            .id(("session-entry", row))
            .debug_selector(move || format!("sessions-row-{row}"))
            .p(px(8.))
            .h(px(height))
            .line_height(px(font.line_height()))
            .rounded(px(crate::config::corners::CONTROL))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(selected, |s| s.bg(rgb(theme.active)))
            .when(spec.checked, |s| s.bg(rgb(theme.primary_wash())))
            .when(spec.enabled, |s| s.cursor_pointer())
            .text_color(rgb(if spec.enabled {
                theme.foreground
            } else {
                theme.muted
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .when(spec.checked, |label| {
                                label
                                    .debug_selector(move || format!("sessions-current-{row}"))
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .child(spec.label.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(font.size * 0.85))
                            .text_color(rgb(theme.muted))
                            .child(if deleting {
                                if self.sessions.mutation_stop {
                                    "Stopping…".into()
                                } else {
                                    "Deleting…".into()
                                }
                            } else {
                                spec.detail.clone()
                            }),
                    )
                    .when_some(spec.context.clone(), |column, context| {
                        column.child(
                            div()
                                .truncate()
                                .text_size(px(font.size * 0.85))
                                .text_color(rgb(theme.muted))
                                .child(context),
                        )
                    })
                    .when_some(badge, |column, badge| {
                        column.child(
                            div()
                                .debug_selector(move || format!("sessions-mr-{row}"))
                                .truncate()
                                .text_size(px(font.size * 0.85))
                                .text_color(rgb(theme.muted))
                                .child(badge),
                        )
                    }),
            )
            .when(removable, |container| {
                container.child(
                    div()
                        .debug_selector(move || format!("sessions-dot-{row}"))
                        .size(px(7.))
                        .flex_none()
                        .rounded_full()
                        .bg(rgb(if spec.online {
                            colors::online(theme)
                        } else {
                            theme.muted
                        })),
                )
            })
            // Every real row has the same columns, status dot, project button,
            // Stop slot, trash, so they line up whether or not a session can
            // be stopped or deleted. The Stop slot is empty for a stopped
            // session.
            .when(removable, |container| {
                let (foreground, surface) = (theme.foreground, theme.surface);
                let hint: SharedString = "Change which project this session belongs to".into();
                container.child(
                    div()
                        .id(("session-project", row))
                        .debug_selector(move || format!("sessions-project-{row}"))
                        .flex_none()
                        .min_w_0()
                        .max_w(px(72.))
                        .truncate()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(crate::config::corners::CONTROL))
                        .border_1()
                        .border_color(rgb(theme.active))
                        .cursor_pointer()
                        .text_size(px(font.size * 0.85))
                        .text_color(rgb(theme.muted))
                        .hover(|s| {
                            s.bg(colors::action_hover(theme))
                                .text_color(rgb(theme.foreground))
                        })
                        .tooltip(move |_, cx| {
                            cx.new(|_| crate::usage::Hint {
                                text: hint.clone(),
                                foreground,
                                surface,
                            })
                            .into()
                        })
                        .child("Project…")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.open_project_move(move_row.clone(), window, cx);
                        })),
                )
            })
            .when(removable, |container| {
                let slot = div().size(px(24.)).flex_none();
                if !spec.online {
                    return container.child(slot);
                }
                container.child(
                    slot.child(
                        crate::menu::action_icon(
                            "icons/stop.svg",
                            format!("sessions-stop-icon-{row}"),
                            if self.session_stop_reason(&stop_row).is_none() {
                                rgb(theme.foreground)
                            } else {
                                rgb(theme.muted)
                            },
                            rgb(theme.foreground),
                        )
                        .id(("session-stop", row))
                        .debug_selector(move || format!("sessions-stop-{row}"))
                        .hover(|s| s.bg(colors::action_hover(theme)))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.confirm_session_stop(
                                    stop_row.clone(),
                                    stop_target.clone(),
                                    window,
                                    cx,
                                );
                            },
                        )),
                    ),
                )
            })
            .when(removable, |container| {
                container.child(
                    crate::menu::action_icon(
                        "icons/trash.svg",
                        format!("sessions-delete-icon-{row}"),
                        deletion_color,
                        rgb(theme.foreground),
                    )
                    .id(("session-delete", row))
                    .debug_selector(move || format!("sessions-delete-{row}"))
                    .hover(|s| s.bg(colors::action_hover(theme)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.confirm_session_delete(
                            removal.clone(),
                            removal_target.clone(),
                            window,
                            cx,
                        );
                    })),
                )
            })
            // Sits in the trash column; the whole row is the click target.
            .when(!removable, |container| {
                let color = rgb(if spec.enabled {
                    theme.foreground
                } else {
                    theme.muted
                });
                container.child(crate::menu::action_icon(
                    "icons/plus.svg",
                    format!("sessions-add-icon-{row}"),
                    color,
                    color,
                ))
            })
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                if *hovered {
                    this.menu.selected = Some(row);
                    this.pin_session(Some(hover_row.clone()));
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                // Keep the painted identity even if the catalog refreshes.
                this.choose_session(choice.clone(), window, cx);
            }));
        if let Some(remaining) = remaining {
            div()
                .debug_selector(move || format!("sessions-departing-{row}"))
                // Keep the anchored popup's height and every row position stable
                // until the fade finishes; then remove the row in one update.
                .h(px(height))
                .flex_none()
                .overflow_hidden()
                .opacity(remaining)
                .child(view)
                .into_any_element()
        } else {
            view.into_any_element()
        }
    }
}

/// The compact MR badge: the bucket, the first pull request number (and how
/// many more), and whether the summary is mixed or stale.
fn mr_badge(mr: &SessionMr) -> String {
    let mut text = mr.bucket.title().to_owned();
    if let Some(first) = mr.numbers.first() {
        text.push_str(&format!(" #{first}"));
        if mr.numbers.len() > 1 {
            text.push_str(&format!(" +{}", mr.numbers.len() - 1));
        }
    }
    if mr.mixed {
        text.push_str(" · mixed");
    }
    if mr.stale {
        text.push_str(" · stale");
    }
    text
}

impl HerdrWindow {
    /// The MR summary of a listed session, from the cache. Only a member of the
    /// shown Window has an endpoint, and the lookup runs only while that Window
    /// groups by MR; otherwise there is nothing true to say and no badge.
    pub(in crate::menu) fn sessions_row_badge(&self, choice: &Row) -> Option<String> {
        self.sessions_row_mr(choice).map(|mr| mr_badge(&mr))
    }

    fn sessions_row_mr(&self, choice: &Row) -> Option<SessionMr> {
        let window = self.containers.active();
        if window.grouping != Grouping::Mr {
            return None;
        }
        let target = match choice {
            Row::Local(name) => session_ref(crate::endpoint::LOCAL, name.as_str()),
            Row::Device { id, session } => session_ref(id, session.as_str()),
            Row::Add(_) => return None,
        };
        if !window.members.contains(&target) {
            return None;
        }
        let index = self
            .endpoints
            .iter()
            .position(|endpoint| endpoint.session_ref() == target)?;
        Some(self.session_mr(index))
    }
}
