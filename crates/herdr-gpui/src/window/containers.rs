//! The window's logical Windows: which session workspaces each lists, which
//! one is shown, and the connection every attached session keeps. Every
//! attached session owns an endpoint, so equal daemon workspace IDs from two
//! sessions never share browser, editor, or activation state.
use crate::{
    HerdrWindow,
    containers::{ContainerError, ContainerSet, SessionRef},
    endpoint::Endpoint,
};
use gpui::Context;
use herdr_client::ConnectTarget;

/// The ID of the endpoint that exists only for `session`.
fn slot_id(session: &SessionRef) -> String {
    format!("slot:{}:{}", session.device, session.session)
}

impl HerdrWindow {
    /// Whether the current logical Window lists endpoint `index`, and the
    /// device filter allows its device.
    pub(crate) fn endpoint_listed(&self, index: usize) -> bool {
        let endpoint = &self.endpoints[index];
        self.device_visible(endpoint.device_id())
            && endpoint.listed_in(&self.containers.active().slots)
    }

    /// Whether the sidebar heads each session: more than one session is listed,
    /// or one that is not a device's own is.
    pub(crate) fn sidebar_multi(&self) -> bool {
        // The device filter narrows the rows, not whether they carry headers.
        let slots = &self.containers.active().slots;
        let mut listed = (0..self.endpoints.len()).filter(|i| self.endpoints[*i].listed_in(slots));
        let first = listed.next();
        listed.next().is_some() || first.is_some_and(|i| self.endpoints[i].slot.is_some())
    }

    /// Whether the shown session runs on this machine's own device.
    pub(crate) fn selected_is_local(&self) -> bool {
        self.endpoints[self.selected_endpoint].device_id() == crate::endpoint::LOCAL
    }

    /// How many endpoints are devices. Session connections follow them, so
    /// every device keeps the index it has in the saved catalog.
    pub(crate) fn device_count(&self) -> usize {
        self.endpoints
            .iter()
            .take_while(|e| e.slot.is_none())
            .count()
    }

    /// Lists every connection's session in the current logical Window, for
    /// fixtures that build their hosts by hand.
    #[cfg(any(test, feature = "integration-test"))]
    pub(crate) fn list_all_endpoints(&mut self) {
        let sessions: Vec<_> = self.endpoints.iter().map(Endpoint::session_ref).collect();
        let active = self.containers.active_index();
        for session in sessions {
            let _ = self.containers.attach(active, session);
        }
    }

    /// The endpoint index that talks to `session`, device or slot alike.
    pub(crate) fn endpoint_of(&self, session: &SessionRef) -> Option<usize> {
        self.endpoints
            .iter()
            .position(|endpoint| endpoint.session_ref() == *session)
    }

    /// The target `session` is reached through, built from its device.
    fn session_target(&self, session: &SessionRef) -> Option<ConnectTarget> {
        let device = self
            .endpoints
            .iter()
            .find(|e| e.slot.is_none() && e.id == session.device)?;
        // A name that is a path is an explicit socket, not a session.
        if herdr_client::session_socket(std::path::Path::new(""), &session.session).is_err() {
            return None;
        }
        match device.saved_target() {
            ConnectTarget::Session { development, .. } => Some(ConnectTarget::Session {
                name: session.session.clone(),
                development: *development,
            }),
            ConnectTarget::Local => Some(ConnectTarget::Session {
                name: session.session.clone(),
                development: false,
            }),
            // An explicit socket names exactly one session.
            ConnectTarget::Socket(_) => None,
            target => target
                .remote_host()
                .map(|host| host.target(session.session.clone())),
        }
    }

    /// Whether `session` can be attached from this window right now.
    pub(crate) fn can_attach(&self, session: &SessionRef) -> bool {
        self.endpoint_of(session).is_some()
            || (self
                .endpoints
                .iter()
                .any(|e| e.slot.is_none() && e.id == session.device && e.enabled)
                && self.session_target(session).is_some())
    }

    /// Creates the connection for every slot whose device is known and has no
    /// endpoint yet. The poll loop dials it; nothing here does I/O.
    pub(crate) fn materialize_slots(&mut self) {
        let wanted: Vec<SessionRef> = self.containers.sessions().cloned().collect();
        for session in wanted {
            if self.endpoint_of(&session).is_some() {
                continue;
            }
            let Some(target) = self.session_target(&session) else {
                continue;
            };
            let Some(device) = self
                .endpoints
                .iter()
                .find(|e| e.slot.is_none() && e.id == session.device)
            else {
                continue;
            };
            let mut endpoint = Endpoint::new(
                slot_id(&session),
                device.label.clone(),
                target,
                device.enabled,
            );
            endpoint.slot = Some(session);
            self.endpoints.push(endpoint);
        }
    }

    /// Drops slots whose device left the catalog. Their connections go with
    /// them; the sessions themselves are untouched.
    pub(crate) fn prune_slots(&mut self) {
        let devices: Vec<String> = self
            .endpoints
            .iter()
            .filter(|e| e.slot.is_none())
            .map(|e| e.id.clone())
            .collect();
        self.containers
            .retain(|slot| devices.contains(&slot.device));
        self.drop_unreferenced_slot_endpoints();
    }

    /// Retires slot endpoints no logical Window lists any longer, keeping the
    /// selected one while it is on screen. The retired connection detaches
    /// from its transport only; the Herdr session keeps running.
    pub(crate) fn drop_unreferenced_slot_endpoints(&mut self) {
        let mut index = self.endpoints.len();
        while index > 0 {
            index -= 1;
            let endpoint = &self.endpoints[index];
            let Some(session) = &endpoint.slot else {
                continue;
            };
            if self.containers.references(session) || index == self.selected_endpoint {
                continue;
            }
            self.endpoints.remove(index);
            if index < self.selected_endpoint {
                self.selected_endpoint -= 1;
            }
        }
    }

    /// Attaches `session` to the current logical Window and shows it. The
    /// session keeps one connection however many Windows list it; a stopped one
    /// is dialled again, which starts it, as picking it always has.
    pub(crate) fn attach_session(&mut self, session: SessionRef, cx: &mut Context<Self>) {
        if self.endpoint_of(&session).is_none() {
            let Some(target) = self.session_target(&session) else {
                self.local_error = Some("This device cannot attach another session".into());
                cx.notify();
                return;
            };
            let Some(device) = self
                .endpoints
                .iter()
                .find(|e| e.slot.is_none() && e.id == session.device && e.enabled)
            else {
                return;
            };
            let mut endpoint = Endpoint::new(
                slot_id(&session),
                device.label.clone(),
                target,
                device.enabled,
            );
            endpoint.slot = Some(session.clone());
            self.endpoints.push(endpoint);
        }
        if let Err(error) = self
            .containers
            .attach(self.containers.active_index(), session.clone())
        {
            self.local_error = Some(error.to_string());
            cx.notify();
            return;
        }
        let Some(index) = self.endpoint_of(&session) else {
            return;
        };
        self.endpoints[index].resume();
        if !self.show_endpoint(index, cx) {
            return;
        }
        self.containers_changed(cx);
        cx.notify();
    }

    /// Makes `index` the selected endpoint. Only a device's own choice is
    /// remembered as the saved host selection; a slot is a view choice.
    fn show_endpoint(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let id = self.endpoints[index].id.clone();
        if self.endpoints[index].slot.is_some() {
            self.switch_endpoint(&id, cx)
        } else {
            self.select_endpoint(&id, cx)
        }
    }

    /// A click on the session row of endpoint `id`.
    pub(crate) fn select_listed(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.endpoints.iter().position(|e| e.id == id) else {
            return;
        };
        let session = self.endpoints[index].session_ref();
        if !self.containers.select(&session) {
            return;
        }
        // Choosing a stopped session again connects it, as picking it always did.
        self.endpoints[index].resume();
        self.show_endpoint(index, cx);
        self.containers_changed(cx);
    }

    /// The × on a session row: view only, never a stop or delete.
    pub(crate) fn remove_listed(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.endpoints.iter().position(|e| e.id == id) {
            let session = self.endpoints[index].session_ref();
            self.remove_slot(&session, cx);
        }
    }

    /// Removes a session workspace row from the current Window. View only: the
    /// session and its daemon are never touched.
    pub(crate) fn remove_slot(&mut self, session: &SessionRef, cx: &mut Context<Self>) {
        let active = self.containers.active_index();
        if !self.containers.detach(active, session) {
            return;
        }
        self.after_container_view_change(cx);
    }

    pub(crate) fn new_container(
        &mut self,
        title: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<usize, ContainerError> {
        let index = self.containers.create(title)?;
        self.after_container_view_change(cx);
        Ok(index)
    }

    pub(crate) fn switch_container(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        if !self.containers.activate(index) {
            return false;
        }
        self.after_container_view_change(cx);
        true
    }

    pub(crate) fn step_container(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.containers.step(forward);
        self.after_container_view_change(cx);
    }

    pub(crate) fn close_container(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), ContainerError> {
        self.containers.close(index)?;
        self.after_container_view_change(cx);
        Ok(())
    }

    /// After the active Window or its membership changed: show the Window's
    /// selected row. An empty Window keeps the previous connection alive but
    /// hidden behind its empty state.
    fn after_container_view_change(&mut self, cx: &mut Context<Self>) {
        let wanted = self.containers.active().selected.clone();
        let target = wanted
            .as_ref()
            .and_then(|session| self.endpoint_of(session))
            .or_else(|| {
                self.containers
                    .active()
                    .slots
                    .iter()
                    .find_map(|session| self.endpoint_of(session))
            });
        if let Some(index) = target
            && index != self.selected_endpoint
        {
            self.show_endpoint(index, cx);
        }
        self.drop_unreferenced_slot_endpoints();
        self.containers_changed(cx);
        cx.notify();
    }

    /// The current Window lists no session, so there is nothing to show.
    pub(crate) fn container_is_empty(&self) -> bool {
        self.containers.active().slots.is_empty()
    }

    /// Hands the new state to the nonblocking writer.
    pub(crate) fn containers_changed(&mut self, cx: &mut Context<Self>) {
        crate::window_state::WindowState::record_containers(self.window_id, &self.containers, cx);
    }

    /// A selection restored from disk waits for its connection to come up.
    pub(crate) fn restore_container_selection(&mut self, cx: &mut Context<Self>) {
        if !self.container_restore_pending {
            return;
        }
        let Some(session) = self
            .containers
            .active()
            .selected
            .clone()
            .or_else(|| self.containers.active().slots.first().cloned())
        else {
            self.container_restore_pending = false;
            return;
        };
        let Some(index) = self.endpoint_of(&session) else {
            // Its device is not in the catalog (yet); keep waiting for it.
            return;
        };
        let endpoint = &self.endpoints[index];
        if index == self.selected_endpoint {
            self.container_restore_pending = false;
            return;
        }
        if endpoint.enabled
            && endpoint.connection.handle.is_some()
            && endpoint.live.status.is_connected()
            && endpoint.live.snapshot.is_some()
        {
            self.container_restore_pending = false;
            self.show_endpoint(index, cx);
        }
    }

    /// Lists the selected endpoint's session in the current logical Window when
    /// it is not there yet, so what is on screen always has a row.
    pub(crate) fn list_selected_session(&mut self) {
        let session = self.endpoints[self.selected_endpoint].session_ref();
        self.containers_dirty = true;
        if self.containers.active().slots.contains(&session) {
            self.containers.select(&session);
            return;
        }
        let active = self.containers.active_index();
        if self.containers.attach(active, session).is_err() {
            self.local_error = Some("This window lists too many sessions".into());
        }
    }

    pub(crate) fn install_containers(&mut self, set: ContainerSet) {
        self.containers = set;
        self.container_restore_pending = true;
        self.materialize_slots();
    }
}

#[cfg(test)]
mod tests;
