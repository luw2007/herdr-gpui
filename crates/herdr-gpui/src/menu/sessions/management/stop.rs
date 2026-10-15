//! Stop keeps a session's saved state and terminates only its running panes.
//! Nothing here deletes the session or any Git worktree its panes used.
use super::*;

impl HerdrWindow {
    /// Why `row` cannot be stopped right now, if it cannot. The root session is
    /// protected exactly as the backend protects it, and only a session the
    /// latest scan saw running is offered: a stopped one has nothing to stop.
    pub(in crate::menu::sessions) fn session_stop_reason(&self, row: &Row) -> Option<&'static str> {
        if self.sessions.mutation.is_some() {
            return Some("Wait for the current session change to finish.");
        }
        let name = match row {
            Row::Local(name) | Row::Device { session: name, .. } => name,
            Row::Add(_) => return Some("Select a session to stop."),
        };
        if name == "default" {
            return Some("Herdr cannot stop the default session.");
        }
        let Some(target) = self.management_target(row) else {
            return Some(
                "Session management is unavailable for explicit sockets, development catalogs, or disabled devices.",
            );
        };
        match target {
            Target::Local => match self.sessions.entries.iter().find(|s| s.name == *name) {
                None => Some("Refresh the session list before stopping."),
                Some(session) if !session.state.running() => Some("This session is not running."),
                Some(_) => None,
            },
            Target::Device { id, .. } => {
                let Some(DeviceScan::Sessions(sessions)) = self.sessions.devices.answers.get(&id)
                else {
                    return Some("Wait for this device's session list before stopping.");
                };
                match sessions.iter().find(|s| s.name == *name) {
                    None => Some("This session is not in the device's latest list."),
                    Some(session) if !session.running => Some("This session is not running."),
                    Some(_) => None,
                }
            }
        }
    }

    pub(in crate::menu::sessions) fn confirm_session_stop(
        &mut self,
        row: Row,
        painted_target: Option<Target>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.management_target(&row) != painted_target {
            self.sessions.mutation_error =
                Some("The device changed. Reopen the session picker.".into());
            cx.notify();
            return;
        }
        if let Some(reason) = self.session_stop_reason(&row) {
            self.sessions.mutation_error = Some(reason.into());
        } else if let Some(target) = self.management_target(&row) {
            self.menu.error = None;
            self.sessions.mutation_error = None;
            self.menu.session_edit = Some(Edit::Stop { row, target });
            window.focus(&self.menu.focus, cx);
        }
        cx.notify();
    }

    pub(super) fn stop_modal_body(&self, view: Div, row: &Row) -> Div {
        let name = match row {
            Row::Local(name) | Row::Device { session: name, .. } => name.as_str(),
            Row::Add(_) => "",
        };
        view.child(
            div()
                .text_size(px(self.config.ui.size * 1.35))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Stop session?"),
        )
        .child(div().min_w_0().truncate().child(name.to_owned()))
        .child(div().text_color(rgb(self.theme.subtext())).child(
            "Herdr will terminate this session's running pane processes. Its saved state is kept, so it can be opened again later, and no Git worktree is touched. Every window of this app stops reconnecting to it until you choose it again; another Herdr client can still start it.",
        ))
    }

    /// Re-check what the confirmation captured, retire every connection of this
    /// app to the session, and stop it on a worker. A failed stop leaves the
    /// connections detached: the outcome may be partial, and reconnecting would
    /// start a session the user asked to stop. Choosing the session again
    /// connects it.
    pub(super) fn submit_session_stop(
        &mut self,
        row: Row,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.management_target(&row).as_ref() != Some(&target) {
            self.menu.error = Some("The device changed. Reopen the session picker.".into());
            cx.notify();
            return;
        }
        if let Some(reason) = self.session_stop_reason(&row) {
            self.menu.error = Some(reason.into());
            cx.notify();
            return;
        }
        let name = match &row {
            Row::Local(name) | Row::Device { session: name, .. } => name.clone(),
            Row::Add(_) => return,
        };
        let connection = match &target {
            Target::Local => ConnectTarget::Session {
                name: name.clone(),
                development: false,
            },
            Target::Device { host, .. } => host.target(name.clone()),
        };
        self.sessions.mutation_error = None;
        self.sessions.mutation_stop = true;
        self.sessions.mutation_target = Some(connection.clone());
        // Pausing the selected endpoint resets the menu. Keep the picker, its
        // search and its selection exactly as the user left them.
        let on_picker = self.menu.page == Some(Page::Sessions);
        let (view, selected) = (self.menu.sessions_view.take(), self.menu.selected);
        let (anchor, scroll) = (self.menu.anchor, self.menu.sessions_scroll.offset());
        let current = window.window_handle().window_id();
        self.pause_session_in_all_windows(&connection, current, cx);
        if on_picker {
            self.menu.page = Some(Page::Sessions);
            self.menu.selected = selected;
            self.menu.sessions_view = view;
            self.menu.anchor = anchor;
            self.menu.sessions_scroll.set_offset(scroll);
            // The reset advanced the selection epoch; the picker is still the
            // same menu, so its captured modal target follows.
            self.menu.endpoint_target = (
                self.selection_epoch,
                self.endpoints[self.selected_endpoint].generation,
            );
        }
        self.cancel_session_edit(window, cx);
        let captured = (row, target.clone());
        let background = cx.background_executor().spawn(async move {
            match target {
                Target::Local => {
                    herdr_client::stop_local_session(&crate::daemon::executable(), &name)
                }
                Target::Device { host, .. } => host.stop_session(&name),
            }
        });
        self.sessions.mutation = Some(cx.spawn(async move |this, cx| {
            let result = background.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_session_stop(result.is_ok(), result.err(), &captured.0, &captured.1);
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl HerdrWindow {
    /// Apply a finished Stop. Only the host this Stop was confirmed for may
    /// show Stopped: a catalog replacement while the worker ran gives the same
    /// id to another host, whose same-named session this did not touch.
    pub(super) fn finish_session_stop(
        &mut self,
        stopped: bool,
        error: Option<herdr_client::Error>,
        row: &Row,
        target: &Target,
    ) {
        self.sessions.mutation = None;
        self.sessions.mutation_target = None;
        self.sessions.mutation_stop = false;
        self.sessions.mutation_error = error.map(|error| error.to_string());
        if stopped
            && self.management_target(row).as_ref() == Some(target)
            && let Row::Local(name) | Row::Device { session: name, .. } = row
        {
            let device = match target {
                Target::Local => None,
                Target::Device { id, .. } => Some(id.as_str()),
            };
            self.sessions.mark_stopped(device, name);
        }
        // Refresh even after a timeout: the CLI may have applied the
        // operation before its answer was lost.
        self.sessions.refresh();
        self.sessions.devices.refresh();
    }
}

impl crate::sessions::Sessions {
    /// Show a session the user just stopped as stopped until the next scan
    /// answers. The entry stays listed: Stop keeps the session.
    fn mark_stopped(&mut self, device: Option<&str>, name: &str) {
        match device {
            None => {
                for session in self.entries.iter_mut().filter(|s| s.name == name) {
                    session.state = herdr_client::SessionState::Stopped;
                }
            }
            Some(id) => {
                if let Some(DeviceScan::Sessions(sessions)) = self.devices.answers.get_mut(id) {
                    for session in sessions.iter_mut().filter(|s| s.name == name) {
                        session.running = false;
                    }
                }
            }
        }
    }
}
