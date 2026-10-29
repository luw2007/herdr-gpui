//! Building the painted lines of the sessions list: this machine's sessions,
//! each saved device's, filtered, grouped and paged.
use super::{Entry, Row, Spec, catalog, management, state_word};
use crate::{HerdrWindow, containers::target_session, endpoint::Endpoint, sessions::DeviceScan};
use herdr_client::{LocalSession, SessionWorkspace};

impl HerdrWindow {
    /// Whether the current logical Window lists this session.
    pub(super) fn session_attached(&self, device: &str, session: &str) -> bool {
        self.containers
            .active()
            .members
            .iter()
            .any(|slot| slot.device.as_str() == device && slot.session == session)
    }

    /// Every painted line, in paint order: the device this window is on leads,
    /// then this machine's sessions, then the devices left over. A search or a
    /// run-state filter hides what does not match, and a device or section with
    /// nothing left is not painted.
    pub(in crate::menu) fn session_entries(&self) -> Vec<Entry> {
        self.session_page_entries().0
    }

    /// The current page of lines and how many pages the matches fill.
    pub(super) fn session_page_entries(&self) -> (Vec<Entry>, usize) {
        // The device the shown session runs on; a session's own connection
        // is not a device of the catalog.
        let current = self
            .endpoints
            .iter()
            .position(|e| {
                e.slot.is_none() && e.id == self.endpoints[self.selected_endpoint].device_id()
            })
            .unwrap_or(0);
        let query = catalog::Query::new(self.session_query());
        let scope = self.session_scope();
        let narrowed = scope.narrows(&query);
        let chosen = scope.facets.device.as_deref();
        let mut entries = Vec::new();
        if let Some(error) = &self.sessions.mutation_error {
            entries.push(Entry::Error(error.clone()));
        }
        // A device choice selects the sections it keeps before any of them is
        // built: the notes, scan errors and Add actions of the others never
        // reach the list, and a kept one still reports its own scan.
        let mut sections: Vec<Vec<Entry>> = Vec::new();
        if let Some(endpoint) = self.endpoints.get(current).filter(|_| current != 0)
            && scope.facets.admits_device(&endpoint.id)
        {
            let mut section = vec![Entry::Header {
                selector: "sessions-current-device".to_owned(),
                text: format!("{} · CURRENT DEVICE", endpoint.label.to_uppercase()),
            }];
            self.device_entries(&mut section, endpoint, &query, &scope);
            sections.push(section);
        }
        if scope.facets.admits_device(&self.endpoints[0].id) {
            let mut local = vec![Entry::Header {
                selector: "sessions-local-header".to_owned(),
                text: "LOCAL SESSIONS".to_owned(),
            }];
            self.local_entries(&mut local, &query, &scope);
            // This machine's scan error belongs to its own section, not to the
            // device sections that follow it.
            if let Some(error) = &self.sessions.error {
                local.push(Entry::Error(error.clone()));
            }
            sections.push(local);
        }
        let others: Vec<usize> = (1..self.endpoints.len())
            .filter(|index| {
                *index != current
                    && self.endpoints[*index].slot.is_none()
                    && scope.facets.admits_device(&self.endpoints[*index].id)
            })
            .collect();
        let mut remote = Vec::new();
        // The group that heads the other devices belongs to them: it is painted
        // only when one of them is kept, or when no device is chosen at all.
        if !others.is_empty() || (current == 0 && chosen.is_none()) {
            remote.push(Entry::Header {
                selector: "sessions-others-header".to_owned(),
                text: if current == 0 {
                    "REMOTE DEVICES".to_owned()
                } else {
                    "OTHER DEVICES".to_owned()
                },
            });
        }
        if others.is_empty() && current == 0 && chosen.is_none() {
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
            self.device_entries(&mut section, endpoint, &query, &scope);
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
        // A search, or a device choice whose device is not in the catalog, can
        // leave nothing at all: say so rather than an empty page.
        if (narrowed || chosen.is_some()) && !entries.iter().any(|entry| entry.row().is_some()) {
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
    pub(super) fn device_entries(
        &self,
        entries: &mut Vec<Entry>,
        endpoint: &Endpoint,
        query: &catalog::Query,
        scope: &catalog::Scope,
    ) {
        let narrowed = scope.narrows(query);
        let saved = target_session(&endpoint.connection.target);
        let fallback = || {
            (
                saved.clone(),
                endpoint.live.status.to_string(),
                endpoint.live.status.is_connected(),
                self.session_attached(&endpoint.id, &saved),
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
                                self.session_attached(&endpoint.id, &session.name),
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
                let project = self.session_project(&endpoint.id, name, workspaces);
                scope.filter.admits(*online)
                    && scope.facets.admits_project(
                        self.session_project_id(&endpoint.id, name, workspaces)
                            .as_ref(),
                    )
                    && query.matches(
                        &endpoint.label,
                        name,
                        *online,
                        project.as_ref().map(|(name, _)| name.as_str()),
                        workspaces,
                    )
            })
            .collect();
        let place = |row: &(String, String, bool, bool, Vec<SessionWorkspace>)| {
            let project = self.session_project(&endpoint.id, &row.0, &row.4);
            catalog::Place::of(project.as_ref().map(|(name, _)| name.as_str()), &row.4)
        };
        let grouped = catalog::group(rows, place);
        for (heading, (name, detail, online, checked, workspaces)) in grouped {
            let detail = self.with_source(detail, &endpoint.id, &name, &workspaces);
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
    pub(super) fn local_entries(
        &self,
        entries: &mut Vec<Entry>,
        query: &catalog::Query,
        scope: &catalog::Scope,
    ) {
        let narrowed = scope.narrows(query);
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
        // With no project anywhere the grouping is invisible, so say where
        // projects come from rather than leaving an unexplained flat list.
        if self.sessions.projects.projects().is_empty() && !narrowed {
            entries.push(Entry::Muted {
                selector: "sessions-no-projects".to_owned(),
                text: "No projects yet — use New project… above, or Project… on a session row, to group sessions.".to_owned(),
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
                scope.filter.admits(session.state.running())
                    && scope.facets.admits_project(
                        self.session_project_id(
                            &self.endpoints[0].id,
                            &session.name,
                            &session.workspaces,
                        )
                        .as_ref(),
                    )
                    && query.matches(
                        device,
                        &session.name,
                        session.state.running(),
                        self.local_project(session)
                            .as_ref()
                            .map(|(name, _)| name.as_str()),
                        &session.workspaces,
                    )
            })
            .collect();
        let place = |session: &&LocalSession| {
            catalog::Place::of(
                self.local_project(session)
                    .as_ref()
                    .map(|(name, _)| name.as_str()),
                &session.workspaces,
            )
        };
        for (heading, session) in catalog::group(shown, place) {
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
    pub(in crate::menu) fn row_children(entries: &[Entry]) -> Vec<usize> {
        entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.row().is_some())
            .map(|(index, _)| index)
            .collect()
    }

    pub(super) fn local_session_spec(
        &self,
        session: &LocalSession,
        query: &catalog::Query,
    ) -> Spec {
        Spec {
            label: session.name.clone().into(),
            detail: self
                .with_source(
                    state_word(session.state.running()).to_owned(),
                    &self.endpoints[0].id,
                    &session.name,
                    &session.workspaces,
                )
                .into(),
            context: catalog::summary(&session.workspaces, query).map(Into::into),
            online: session.state.running(),
            checked: self.session_attached(crate::endpoint::LOCAL, &session.name),
            enabled: true,
        }
    }
}
