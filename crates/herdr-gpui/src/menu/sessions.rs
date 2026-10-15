//! The sessions popup: the named sessions on this machine and the ones each
//! saved device reports, grouped under the device they run on and led by the
//! device this window is on.
//! Both lists are probes — a session socket outlives its daemon, and a device's
//! sessions cost an SSH round trip — so this page is the only thing that asks.
use super::{Page, colors, devices};
use crate::{HerdrWindow, endpoint::Endpoint, sessions::DeviceScan};
use gpui::{prelude::*, *};
use herdr_client::{ConnectTarget, LocalSession, SessionWorkspace};
mod catalog;
mod management;
mod view;
pub(super) use catalog::Filter;
pub(super) use management::Edit;
pub(super) use view::View;

/// The manager is wide: a row carries a project, a branch and a state.
pub(super) const SESSIONS_WIDTH: f32 = 440.;

/// A line in the popup, in the order it is painted. Only a row can be selected,
/// so navigation walks the `Row` entries and skips headers and notes.
#[derive(Clone)]
pub(super) enum Entry {
    /// A section title. It carries a selector because the order of the sections
    /// is what says which device the list is leading with, and a device's own
    /// title is named after it.
    Header {
        selector: String,
        text: String,
    },
    /// A muted note, carrying the selector diagnostics and tests look for.
    Muted {
        selector: String,
        text: String,
    },
    Error(String),
    /// A selectable line, carrying what it paints: a row resolved again at paint
    /// time could describe a session its own scan has already replaced.
    Row {
        row: Row,
        spec: Spec,
    },
}

/// What a row attaches to. A device keeps its endpoint id rather than an index,
/// so a catalog change between paint and click cannot pick the wrong one, and it
/// names the session itself, because a device has many.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Local(String),
    Device { id: String, session: String },
    Add(management::Target),
}

/// What one row paints. Both kinds share one shape so they cannot drift apart.
#[derive(Clone)]
pub(super) struct Spec {
    label: SharedString,
    detail: SharedString,
    /// The saved workspace context line, when the session has any.
    context: Option<SharedString>,
    /// Running locally, or connected remotely.
    online: bool,
    checked: bool,
    enabled: bool,
}

impl Row {
    /// A real session, as opposed to the Add action.
    fn is_session(&self) -> bool {
        !matches!(self, Self::Add(_))
    }
}

impl Entry {
    pub(super) fn row(&self) -> Option<&Row> {
        match self {
            Self::Row { row, .. } => Some(row),
            _ => None,
        }
    }
}

/// The session a saved device attaches to. Asking the host what else it runs
/// would be an SSH round trip this popup does not take.
fn target_session(target: &ConnectTarget) -> String {
    match target {
        ConnectTarget::Ssh { session, .. }
        | ConnectTarget::Wsl { session, .. }
        | ConnectTarget::Session { name: session, .. } => session.clone(),
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud { session, .. } => session.clone(),
        ConnectTarget::Socket(path) => path.display().to_string(),
        ConnectTarget::Local => "default".to_owned(),
    }
}

/// The state word a session paints, whether it was probed on this machine or
/// reported by its host.
fn state_word(running: bool) -> &'static str {
    if running { "running" } else { "stopped" }
}

impl HerdrWindow {
    pub(super) fn finish_session_departure(&mut self) {
        // Selection is an ordinal shared with keyboard navigation. Once a row
        // disappears it must not transfer to the action now occupying its slot.
        if self.sessions.departure.is_some() && self.menu.page == Some(Page::Sessions) {
            self.menu.selected = None;
        }
        self.sessions.finish_departure();
    }

    /// Open the local and remote session list above the footer button.
    pub(crate) fn open_sessions(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open_menu(window, cx) {
            return;
        }
        // Anchor to the control, not the pointer: every click position leaves the
        // same clear gap above the button.
        self.menu.anchor = anchor;
        // Probe again on every open. The list from the last open paints at once,
        // so the popup is never blank while the new scan runs.
        self.sessions.refresh();
        self.menu.page = Some(Page::Sessions);
        self.open_sessions_view(window, cx);
        self.pin_first_session();
    }

    /// The socket this window's local endpoint would dial. `Local` resolves
    /// through the environment, so no session name can say which one is current.
    fn local_socket(&self) -> Option<std::path::PathBuf> {
        self.endpoints[0].connection.target.socket_path().ok()
    }

    /// Every painted line, in paint order: the device this window is on leads,
    /// then this machine's sessions, then the devices left over. A search or a
    /// run-state filter hides what does not match, and a device or section with
    /// nothing left is not painted.
    pub(super) fn session_entries(&self) -> Vec<Entry> {
        self.session_page_entries().0
    }

    /// The current page of lines and how many pages the matches fill.
    fn session_page_entries(&self) -> (Vec<Entry>, usize) {
        let current = self.selected_endpoint;
        let query = catalog::Query::new(self.session_query());
        let filter = self.session_filter();
        let narrowed = !query.is_empty() || filter != Filter::All;
        let mut entries = Vec::new();
        if let Some(error) = &self.sessions.mutation_error {
            entries.push(Entry::Error(error.clone()));
        }
        let mut sections: Vec<Vec<Entry>> = Vec::new();
        if let Some(endpoint) = self.endpoints.get(current).filter(|_| current != 0) {
            let mut section = vec![Entry::Header {
                selector: "sessions-current-device".to_owned(),
                text: format!("{} · CURRENT DEVICE", endpoint.label.to_uppercase()),
            }];
            self.device_entries(&mut section, endpoint, current, &query, filter);
            sections.push(section);
        }
        let mut local = vec![Entry::Header {
            selector: "sessions-local-header".to_owned(),
            text: "LOCAL SESSIONS".to_owned(),
        }];
        self.local_entries(&mut local, &query, filter);
        // This machine's scan error belongs to its own section, not to the device
        // sections that follow it.
        if let Some(error) = &self.sessions.error {
            local.push(Entry::Error(error.clone()));
        }
        sections.push(local);
        let others: Vec<usize> = (1..self.endpoints.len())
            .filter(|index| *index != current)
            .collect();
        let mut remote = Vec::new();
        if !others.is_empty() || current == 0 {
            remote.push(Entry::Header {
                selector: "sessions-others-header".to_owned(),
                text: if current == 0 {
                    "REMOTE DEVICES".to_owned()
                } else {
                    "OTHER DEVICES".to_owned()
                },
            });
        }
        if others.is_empty() && current == 0 {
            remote.push(Entry::Muted {
                selector: "sessions-no-devices".to_owned(),
                text: "No saved devices. Add one from the device picker.".to_owned(),
            });
        }
        // Each device names itself: the rows under it are its sessions, not the
        // host and target that used to be the whole of one.
        let mut devices = Vec::new();
        for index in others {
            let endpoint = &self.endpoints[index];
            let mut section = vec![Entry::Header {
                selector: format!("sessions-device-{}", endpoint.id),
                text: endpoint.label.to_uppercase(),
            }];
            self.device_entries(&mut section, endpoint, index, &query, filter);
            devices.push(section);
        }
        let has_session = |section: &[Entry]| {
            section
                .iter()
                .any(|entry| entry.row().is_some_and(Row::is_session))
        };
        for section in sections {
            if !narrowed || has_session(&section) {
                entries.extend(section);
            }
        }
        if !narrowed {
            entries.extend(remote);
        }
        for section in devices {
            if !narrowed || has_session(&section) {
                entries.extend(section);
            }
        }
        if narrowed && !entries.iter().any(|entry| entry.row().is_some()) {
            entries.push(Entry::Muted {
                selector: "sessions-no-matches".to_owned(),
                text: "No sessions match. Try fewer words or choose All.".to_owned(),
            });
        }
        // Pages hold a bounded number of sessions. A continued page opens with
        // the device and project headings its first session sits under, so it
        // is never anonymous, and every session stays reachable.
        let total = entries
            .iter()
            .filter(|entry| entry.row().is_some_and(Row::is_session))
            .count();
        let pages = total.div_ceil(catalog::PAGE_SIZE).max(1);
        let page = self.session_page().min(pages - 1);
        let mut shown: Vec<Entry> = Vec::new();
        let mut seen = 0;
        let mut device: Option<Entry> = None;
        let mut project: Option<Entry> = None;
        let mut pending: Vec<Entry> = Vec::new();
        let mut started = false;
        for entry in entries {
            if entry.row().is_some_and(Row::is_session) {
                if seen / catalog::PAGE_SIZE == page {
                    if !started && page > 0 {
                        shown.extend(device.clone());
                        shown.extend(project.clone());
                        pending.clear();
                    } else {
                        shown.append(&mut pending);
                    }
                    started = true;
                    shown.push(entry);
                } else {
                    pending.clear();
                }
                seen += 1;
                continue;
            }
            match &entry {
                Entry::Header { .. } => {
                    device = Some(entry.clone());
                    project = None;
                }
                Entry::Muted { selector, .. } if selector.starts_with("sessions-project-") => {
                    project = Some(entry.clone());
                }
                _ => {}
            }
            pending.push(entry);
        }
        if page + 1 == pages {
            shown.append(&mut pending);
        }
        (shown, pages)
    }

    /// One saved device's sessions, under the device's own title. The session it
    /// was saved with leads, and stands in for the list while that is still
    /// arriving or could not be read at all, so a host is never unreachable from
    /// this popup just because its session list is.
    fn device_entries(
        &self,
        entries: &mut Vec<Entry>,
        endpoint: &Endpoint,
        index: usize,
        query: &catalog::Query,
        filter: Filter,
    ) {
        let narrowed = !query.is_empty() || filter != Filter::All;
        let saved = target_session(&endpoint.connection.target);
        let attached = self.selected_endpoint == index;
        let fallback = || {
            (
                saved.clone(),
                endpoint.live.status.to_string(),
                endpoint.live.status.is_connected(),
                attached,
                Vec::<SessionWorkspace>::new(),
            )
        };
        let (rows, note) = match self.sessions.devices.answers.get(&endpoint.id) {
            Some(DeviceScan::Sessions(sessions)) if !sessions.is_empty() => {
                let mut rows = Vec::with_capacity(sessions.len() + 1);
                // The session this device attaches to stays selectable even when
                // the host no longer lists it: the picker offers it too, because
                // the daemon starts a named session on demand.
                if !sessions.iter().any(|session| session.name == saved) {
                    rows.push(fallback());
                }
                rows.extend(
                    sessions
                        .iter()
                        .filter(|session| session.name == saved)
                        .chain(sessions.iter().filter(|session| session.name != saved))
                        .map(|session| {
                            (
                                session.name.clone(),
                                state_word(session.running).to_owned(),
                                session.running,
                                attached && session.name == saved,
                                session.workspaces.clone(),
                            )
                        }),
                );
                (rows, None)
            }
            Some(DeviceScan::Sessions(_)) => (
                vec![fallback()],
                Some("No sessions on this device.".to_owned()),
            ),
            Some(DeviceScan::Failed(error)) => (vec![fallback()], Some(error.clone())),
            // A disabled device is never dialled, so it must not claim to be
            // waiting for an answer that is not coming.
            None if !endpoint.enabled => (vec![fallback()], Some("Disabled".to_owned())),
            None => (vec![fallback()], Some("Checking…".to_owned())),
        };
        let rows: Vec<_> = rows
            .into_iter()
            .filter(|(name, _, online, _, workspaces)| {
                filter.admits(*online) && query.matches(&endpoint.label, name, *online, workspaces)
            })
            .collect();
        let grouped = catalog::group(rows, |row| row.4.as_slice());
        for (heading, (name, detail, online, checked, workspaces)) in grouped {
            if let Some(heading) = heading {
                entries.push(Entry::Muted {
                    selector: format!("sessions-project-{}-{heading}", endpoint.id),
                    text: heading,
                });
            }
            entries.push(Entry::Row {
                row: Row::Device {
                    id: endpoint.id.clone(),
                    session: name.clone(),
                },
                spec: Spec {
                    label: name.into(),
                    detail: detail.into(),
                    context: catalog::summary(&workspaces, query).map(Into::into),
                    online,
                    checked,
                    enabled: endpoint.enabled,
                },
            });
        }
        if !narrowed {
            if let Some(note) = note {
                entries.push(Entry::Muted {
                    selector: format!("sessions-device-note-{}", endpoint.id),
                    text: note,
                });
            }
            if let Some(host) = endpoint.connection.target.remote_host() {
                entries.push(self.add_session_entry(management::Target::Device {
                    id: endpoint.id.clone(),
                    host,
                }));
            }
        }
    }

    /// This machine's sessions, with the notes that explain an empty list and what
    /// selecting a stopped session does.
    fn local_entries(&self, entries: &mut Vec<Entry>, query: &catalog::Query, filter: Filter) {
        let narrowed = !query.is_empty() || filter != Filter::All;
        if self.sessions.entries.is_empty() && !narrowed {
            entries.push(Entry::Muted {
                selector: "sessions-empty".to_owned(),
                text: if self.sessions.scanning {
                    "Checking…".to_owned()
                } else {
                    "No sessions on this machine.".to_owned()
                },
            });
        }
        // A stopped session is still worth selecting: it starts its daemon, the
        // same way `herdr --session <name>` does.
        if !narrowed
            && self
                .sessions
                .entries
                .iter()
                .any(|session| !session.state.running())
        {
            entries.push(Entry::Muted {
                selector: "sessions-stopped".to_owned(),
                text: "Stopped sessions start their daemon when selected.".to_owned(),
            });
        }
        let device = self.endpoints[0].label.as_str();
        let shown: Vec<&LocalSession> = self
            .sessions
            .entries
            .iter()
            .filter(|session| {
                filter.admits(session.state.running())
                    && query.matches(
                        device,
                        &session.name,
                        session.state.running(),
                        &session.workspaces,
                    )
            })
            .collect();
        for (heading, session) in catalog::group(shown, |session| session.workspaces.as_slice()) {
            if let Some(heading) = heading {
                entries.push(Entry::Muted {
                    selector: format!("sessions-project-local-{heading}"),
                    text: heading,
                });
            }
            entries.push(Entry::Row {
                row: Row::Local(session.name.clone()),
                spec: self.local_session_spec(session, query),
            });
        }
        if !narrowed {
            entries.push(self.add_session_entry(management::Target::Local));
        }
    }

    /// The child index of each selectable row, which is what navigation and the
    /// scroll handle both address.
    pub(super) fn row_children(entries: &[Entry]) -> Vec<usize> {
        entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.row().is_some())
            .map(|(index, _)| index)
            .collect()
    }

    fn local_session_spec(&self, session: &LocalSession, query: &catalog::Query) -> Spec {
        Spec {
            label: session.name.clone().into(),
            detail: state_word(session.state.running()).into(),
            context: catalog::summary(&session.workspaces, query).map(Into::into),
            online: session.state.running(),
            checked: self
                .local_socket()
                .is_some_and(|socket| socket == session.socket),
            enabled: true,
        }
    }

    fn sessions_row(
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
        let height = font.line_height() * if spec.context.is_some() { 3. } else { 2. } + 16.;
        let removal = choice.clone();
        let removal_target = self.management_target(&choice);
        let removable = !matches!(choice, Row::Add(_));
        let hover_row = choice.clone();
        let stop_row = choice.clone();
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
            // Every real row has the same columns, status dot, Stop slot, trash,
            // so they line up whether or not a session can be stopped or deleted.
            // The slot is empty for a stopped session.
            .when(removable, |container| {
                let slot = div().size(px(24.)).flex_none();
                if !spec.online {
                    return container.child(slot);
                }
                container.child(
                    slot.child(
                        super::action_icon(
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
                    super::action_icon(
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
                container.child(super::action_icon(
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

    pub(super) fn render_sessions(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.menu.session_edit.is_some() {
            return self.render_session_edit(cx).into_any_element();
        }
        self.render_session_list(cx)
    }

    pub(super) fn render_session_list(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .max_h(devices::list_height(self.menu.anchor.y) + px(120.))
            .child(self.render_session_toolbar(cx))
            .child(self.session_rows(cx))
            .into_any_element()
    }

    fn session_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = &self.theme;
        let font = &self.config.ui;
        let entries = self.session_entries();
        let children = Self::row_children(&entries);
        let selected = self.menu.selected;
        let mut view = div()
            .id("sessions-scroll")
            .debug_selector(|| "sessions-list".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.menu.sessions_scroll)
            .flex()
            .flex_col()
            .gap(px(4.));
        for (child, entry) in entries.iter().enumerate() {
            let ordinal = children.iter().position(|index| *index == child);
            view = match entry {
                Entry::Header { selector, text } => {
                    let selector = selector.clone();
                    view.child(
                        div()
                            .debug_selector(move || selector.clone())
                            .p(px(8.))
                            .text_color(rgb(theme.muted))
                            .child(text.clone()),
                    )
                }
                Entry::Muted { selector, text } => {
                    let selector = selector.clone();
                    view.child(
                        div()
                            .debug_selector(move || selector.clone())
                            .px(px(8.))
                            .py(px(2.))
                            .text_size(px(font.size * 0.85))
                            .text_color(rgb(theme.muted))
                            .child(text.clone()),
                    )
                }
                Entry::Error(text) => view.child(
                    div()
                        .debug_selector(|| "sessions-error".into())
                        .px(px(8.))
                        .py(px(2.))
                        .text_size(px(font.size * 0.85))
                        .text_color(colors::danger(theme))
                        .child(text.clone()),
                ),
                Entry::Row { row, spec } => view.child(self.sessions_row(
                    ordinal.unwrap_or_default(),
                    row.clone(),
                    ordinal == selected,
                    spec,
                    cx,
                )),
            };
        }
        view.into_any_element()
    }

    /// Attach to a row's session. The row is what the caller painted, not an
    /// index into a list computed again here: a device list that refreshed in
    /// between would otherwise have the click name a different session.
    fn choose_session(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        if self.sessions.mutation.is_some() {
            return;
        }
        match row {
            Row::Add(target) => self.open_session_create(target, window, cx),
            Row::Local(name) => {
                self.dismiss_menu(window, cx);
                self.select_local_session(&name, cx);
                // The picker's own follow-up: the sidebar follows the host that
                // was just chosen, and another session's tree is not this one's.
                self.device_filter = Some(self.endpoints[0].id.clone());
                for scroll in &self.sidebar_scroll {
                    scroll.set_offset(Point::default());
                }
                for revealed in &self.sidebar_revealed {
                    revealed.set(None);
                }
                // The row a reveal left for the next frame is in the old list.
                self.sidebar_pin_reveal.set(None);
            }
            // A device row is one of that device's sessions, so it retargets the
            // device rather than adding an endpoint for every session it runs.
            Row::Device { id, session } => {
                // A disabled device has nothing to attach to, and the picker
                // refuses the same click. Filtering the sidebar to it would leave
                // the host list empty, so the menu stays as it is.
                if !self
                    .endpoints
                    .iter()
                    .any(|endpoint| endpoint.id == id && endpoint.enabled)
                {
                    return;
                }
                self.dismiss_menu(window, cx);
                self.select_device_session(&id, &session, cx);
                // The local branch's own follow-up: the sidebar follows the host
                // that was just chosen, and another session's tree is not this
                // one's.
                self.device_filter = Some(id);
                for scroll in &self.sidebar_scroll {
                    scroll.set_offset(Point::default());
                }
                for revealed in &self.sidebar_revealed {
                    revealed.set(None);
                }
                // The row a reveal left for the next frame is in the old list.
                self.sidebar_pin_reveal.set(None);
            }
        }
    }

    pub(super) fn sessions_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(&self.menu.session_edit, Some(Edit::Create { input, .. })
            if input.read(cx).is_composing() || !matches!(event.keystroke.key.as_str(), "enter" | "escape"))
        {
            return;
        }
        // Everything else, Backspace and Delete included, belongs to the search
        // field: it edits text and can never reach a destructive action. The
        // field takes focus back when a closed modal left it on the menu.
        let search_focused = self
            .menu
            .sessions_view
            .as_ref()
            .is_some_and(|view| view.search.read(cx).focus.is_focused(window));
        if self.menu.session_edit.is_none() && !search_focused {
            self.focus_session_search(window, cx);
        }
        let own = matches!(
            event.keystroke.key.as_str(),
            "up" | "down" | "enter" | "escape" | "tab" | "pagedown" | "pageup"
        );
        if self.menu.session_edit.is_none() && !own {
            return;
        }
        // An IME composition owns Enter, arrows and Escape until it commits.
        if self.menu.session_edit.is_none()
            && self
                .menu
                .sessions_view
                .as_ref()
                .is_some_and(|view| view.search.read(cx).is_composing())
        {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
        if self.menu.session_edit.is_some() {
            self.session_edit_key(event, window, cx);
            return;
        }
        let entries = self.session_entries();
        let children = Self::row_children(&entries);
        match event.keystroke.key.as_str() {
            "up" | "down" if !children.is_empty() => {
                let count = children.len();
                let up = event.keystroke.key == "up";
                let index = match self.menu.selected {
                    Some(index) => (index.min(count - 1) + if up { count - 1 } else { 1 }) % count,
                    None if up => count - 1,
                    None => 0,
                };
                self.menu.selected = Some(index);
                let pinned = children
                    .get(index)
                    .and_then(|child| entries.get(*child))
                    .and_then(Entry::row)
                    .cloned();
                self.pin_session(pinned);
                self.menu.sessions_scroll.scroll_to_item(children[index]);
                cx.notify();
            }
            "enter" if !children.is_empty() => {
                let row = self
                    .menu
                    .selected
                    .and_then(|selected| children.get(selected))
                    .and_then(|child| entries.get(*child))
                    .and_then(Entry::row)
                    .cloned();
                if let Some(row) = row {
                    self.choose_session(row, window, cx);
                }
            }
            "escape" => self.dismiss_menu(window, cx),
            "tab" => {
                let next = self.session_filter().next();
                self.set_session_filter(next, cx);
            }
            "pagedown" | "pageup" => {
                let (_, pages) = self.session_page_entries();
                let page = self.session_page().min(pages - 1);
                let target = if event.keystroke.key == "pagedown" {
                    (page + 1).min(pages - 1)
                } else {
                    page.saturating_sub(1)
                };
                self.set_session_page(target, cx);
            }
            _ => {}
        }
    }
}
