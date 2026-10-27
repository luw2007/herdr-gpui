//! The native window's side of the global Windows: which one it shows, and the
//! connection every attached session keeps. Edits go through the shared
//! registry (`containers::Containers`) and other native windows follow its
//! revision. Every
//! attached session owns an endpoint, so equal daemon workspace IDs from two
//! sessions never share browser, editor, or activation state.
use crate::{
    HerdrWindow,
    containers::{ContainerError, ContainerId, ContainerSet, Containers},
    endpoint::Endpoint,
};
use gpui::Context;
use herdr_client::{ConnectTarget, SessionRef};

/// The connection that exists only for `session`, reached through `device`.
fn slot_endpoint(session: &SessionRef, device: &Endpoint, target: ConnectTarget) -> Endpoint {
    let mut endpoint = Endpoint::new(
        crate::endpoint::slot_endpoint_id(session),
        device.label.clone(),
        target,
        device.enabled,
    );
    endpoint.slot = Some(session.clone());
    endpoint
}

/// Why an attach was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum AttachError {
    #[error("This device cannot attach another session")]
    NoTarget,
    #[error("The device is disabled")]
    Disabled,
    #[error(transparent)]
    Container(#[from] ContainerError),
}

impl HerdrWindow {
    /// The user chose `session` again: it dials, here and after a restart.
    pub(crate) fn resume_session(&mut self, session: &SessionRef, cx: &mut Context<Self>) {
        if let Some(index) = self.endpoint_of(session) {
            self.endpoints[index].resume();
        }
        let _ = self.edit_containers(cx, |set| {
            set.resume(session);
            Ok(())
        });
    }

    /// Whether the current logical Window lists endpoint `index`, and the
    /// device filter allows its device.
    pub(crate) fn endpoint_listed(&self, index: usize) -> bool {
        let endpoint = &self.endpoints[index];
        self.device_visible(endpoint.device_id())
            && endpoint.listed_in(&self.containers.active().members)
    }

    /// Whether the sidebar heads each session: more than one session is listed,
    /// or one that is not a device's own is.
    pub(crate) fn sidebar_multi(&self) -> bool {
        // The device filter narrows the rows, not whether they carry headers.
        let slots = &self.containers.active().members;
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

    /// Lists every connection's session in the shown Window, for
    /// fixtures that build their hosts by hand.
    #[cfg(any(test, all(feature = "integration-test", target_os = "macos")))]
    pub(crate) fn list_all_endpoints(&mut self) {
        let sessions: Vec<_> = self.endpoints.iter().map(Endpoint::session_ref).collect();
        let shown = self.containers.shown_id();
        // Fixtures do not share their Windows: the edit stays in this copy.
        let _ = self.containers.edit_local(|set| {
            for session in sessions {
                set.attach(shown, session)?;
            }
            Ok(())
        });
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
            .find(|e| e.slot.is_none() && e.id == session.device.as_str())?;
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

    #[cfg(unix)]
    /// Whether `session` can be attached from this window right now.
    pub(crate) fn can_attach(&self, session: &SessionRef) -> bool {
        self.endpoint_of(session).is_some()
            || (self
                .endpoints
                .iter()
                .any(|e| e.slot.is_none() && e.id == session.device.as_str() && e.enabled)
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
                .find(|e| e.slot.is_none() && e.id == session.device.as_str())
            else {
                continue;
            };
            let mut endpoint = slot_endpoint(&session, device, target);
            if self.containers.set().is_paused(&session) {
                // Stopped before the last exit: listed, never dialled, until
                // the user chooses it again.
                endpoint.park();
            }
            self.endpoints.push(endpoint);
        }
        self.debug_assert_endpoint_ids_unique();
    }

    /// Drops members whose device left the catalog, in every Window. Their connections go with
    /// them; the sessions themselves are untouched.
    pub(crate) fn prune_slots(&mut self, cx: &mut Context<Self>) {
        let devices: Vec<String> = self
            .endpoints
            .iter()
            .filter(|e| e.slot.is_none())
            .map(|e| e.id.clone())
            .collect();
        let _ = self.edit_containers(cx, |set| {
            set.retain(|member| devices.iter().any(|d| d == member.device.as_str()));
            Ok(())
        });
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
    /// Applies `change` to the shared registry and brings this window's copy up
    /// to date. Every native window follows the new revision on its next tick.
    pub(crate) fn edit_containers<R>(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ContainerSet) -> Result<R, ContainerError>,
    ) -> Result<R, ContainerError> {
        if !cx.has_global::<Containers>() {
            // No shared registry (a fixture): the window's own copy is all
            // there is.
            return self.containers.edit_local(change);
        }
        let result = Containers::update(cx, |store| store.mutate(change));
        self.pull_containers(cx);
        result
    }

    /// Copies the registry when its revision moved. Returns whether it did.
    fn pull_containers(&mut self, cx: &mut Context<Self>) -> bool {
        if cx
            .try_global::<Containers>()
            .is_some_and(Containers::has_external_edits)
        {
            Containers::update(cx, Containers::adopt_external);
        }
        if cx
            .try_global::<crate::projects::Projects>()
            .is_some_and(crate::projects::Projects::has_external_edits)
        {
            crate::projects::Projects::update(cx, crate::projects::Projects::adopt_external);
            self.sync_projects(cx);
        }
        let Some(store) = cx.try_global::<Containers>() else {
            return false;
        };
        let revision = store.revision();
        if revision == self.containers.revision() {
            return false;
        }
        self.containers.mirror(store.set().clone(), revision);
        true
    }

    /// Follows edits another native window (or the CLI) made to the shared
    /// registry. This window keeps showing what it shows: its endpoints are its
    /// own, and only a session it shows being removed from the shown Window
    /// moves the view.
    pub(crate) fn sync_containers(&mut self, cx: &mut Context<Self>) {
        if !self.pull_containers(cx) {
            return;
        }
        self.materialize_slots();
        let shown = self.endpoints[self.selected_endpoint].session_ref();
        let gone = !self.container_is_empty() && !self.containers.active().members.contains(&shown);
        if gone {
            self.after_container_view_change(cx);
        } else {
            self.drop_unreferenced_slot_endpoints();
            self.containers_changed(cx);
        }
        cx.notify();
    }

    /// Attaches `session` to the shown Window and shows it. The session keeps
    /// one connection however many Windows list it; a stopped one is dialled
    /// again, which starts it, as picking it always has. A refusal is shown
    /// as a toast and ignored here; see `attach_shown` for the result.
    pub(crate) fn attach_session(&mut self, session: SessionRef, cx: &mut Context<Self>) {
        let _ = self.attach_shown(session, cx);
    }

    /// `attach_session`, reporting why a refused attach was refused.
    pub(crate) fn attach_shown(
        &mut self,
        session: SessionRef,
        cx: &mut Context<Self>,
    ) -> Result<(), AttachError> {
        let shown = self.containers.shown_id();
        match self.attach_session_to(shown, session.clone(), cx) {
            Ok(()) => {
                self.show_attached(&session, cx);
                Ok(())
            }
            Err(error) => {
                // A disabled device was always refused without a message.
                if error != AttachError::Disabled {
                    self.local_error = Some(error.to_string());
                    cx.notify();
                }
                Err(error)
            }
        }
    }

    /// Lists `session` in Window `id`, creating this window's connection for
    /// it. Reports nothing to the user; the caller decides.
    pub(crate) fn attach_session_to(
        &mut self,
        id: ContainerId,
        session: SessionRef,
        cx: &mut Context<Self>,
    ) -> Result<(), AttachError> {
        // The member is attached before any connection exists, so a refusal
        // (too many sessions) leaves nothing dialling in the background.
        let slot = if self.endpoint_of(&session).is_none() {
            let target = self.session_target(&session).ok_or(AttachError::NoTarget)?;
            let device = self
                .endpoints
                .iter()
                .find(|e| e.slot.is_none() && e.id == session.device.as_str() && e.enabled)
                .ok_or(AttachError::Disabled)?;
            Some(slot_endpoint(&session, device, target))
        } else {
            None
        };
        self.edit_containers(cx, |set| set.attach(id, session).map(drop))?;
        if let Some(endpoint) = slot {
            self.endpoints.push(endpoint);
            self.debug_assert_endpoint_ids_unique();
        }
        Ok(())
    }

    fn show_attached(&mut self, session: &SessionRef, cx: &mut Context<Self>) {
        let Some(index) = self.endpoint_of(session) else {
            return;
        };
        self.resume_session(session, cx);
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
        let shown = self.containers.shown_id();
        if !self
            .edit_containers(cx, |set| Ok(set.select(shown, &session)))
            .unwrap_or(false)
        {
            return;
        }
        // Choosing a stopped session again connects it, as picking it always did.
        self.resume_session(&session, cx);
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

    /// Removes a session workspace row from the shown Window. View only: the
    /// session and its daemon are never touched.
    pub(crate) fn remove_slot(&mut self, session: &SessionRef, cx: &mut Context<Self>) {
        let shown = self.containers.shown_id();
        self.detach_session_from(shown, session, cx);
    }

    /// Removes `session` from Window `id`; whether it was listed there.
    pub(crate) fn detach_session_from(
        &mut self,
        id: ContainerId,
        session: &SessionRef,
        cx: &mut Context<Self>,
    ) -> bool {
        let removed = self
            .edit_containers(cx, |set| Ok(set.detach(id, session)))
            .unwrap_or(false);
        if removed {
            self.after_container_view_change(cx);
        }
        removed
    }

    /// Adds an empty Window to the registry and shows it here.
    pub(crate) fn new_container(
        &mut self,
        title: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<ContainerId, ContainerError> {
        let id = self.edit_containers(cx, |set| set.create(title))?;
        self.containers.show(id);
        self.after_container_view_change(cx);
        Ok(id)
    }

    pub(crate) fn rename_container(
        &mut self,
        id: ContainerId,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), ContainerError> {
        self.edit_containers(cx, |set| set.rename(id, title))?;
        self.containers_changed(cx);
        cx.notify();
        Ok(())
    }

    /// Shows Window `id` in this native window only.
    pub(crate) fn switch_container(&mut self, id: ContainerId, cx: &mut Context<Self>) -> bool {
        if !self.containers.show(id) {
            return false;
        }
        self.after_container_view_change(cx);
        true
    }

    pub(crate) fn step_container(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.containers.step(forward);
        self.after_container_view_change(cx);
    }

    /// Closes Window `id` for every native window. The last one is replaced by
    /// a fresh empty default. Sessions are untouched.
    pub(crate) fn close_container(
        &mut self,
        id: ContainerId,
        cx: &mut Context<Self>,
    ) -> Result<(), ContainerError> {
        self.edit_containers(cx, |set| set.close(id))?;
        self.after_container_view_change(cx);
        Ok(())
    }

    /// After the shown Window or its membership changed: show the Window's
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
                    .members
                    .iter()
                    .find_map(|session| self.endpoint_of(session))
            });
        if let Some(index) = target
            && index != self.selected_endpoint
        {
            self.show_endpoint(index, cx);
        }
        self.materialize_slots();
        self.drop_unreferenced_slot_endpoints();
        self.containers_changed(cx);
        cx.notify();
    }

    /// The shown Window lists no session, so there is nothing to show.
    pub(crate) fn container_is_empty(&self) -> bool {
        self.containers.active().members.is_empty()
    }

    /// Publishes what this window changed: session renames from a deletion
    /// reach the shared registry, and the shown Window is saved for restart.
    pub(crate) fn containers_changed(&mut self, cx: &mut Context<Self>) {
        let rewrites = std::mem::take(&mut self.container_rewrites);
        if !rewrites.is_empty() {
            let _ = self.edit_containers(cx, |set| {
                for (device, from, to) in &rewrites {
                    set.replace_session(device, from, to);
                }
                Ok(())
            });
        }
        crate::window_state::WindowState::record_shown(
            self.window_id,
            self.containers.shown_id(),
            cx,
        );
    }

    /// After the shown connection was retired: points the screen at the shown
    /// Window's selected member again, or at its first usable member, and
    /// keeps waiting to return to the selected one if that could not be shown.
    pub(crate) fn reselect_active_member(&mut self, cx: &mut Context<Self>) {
        self.container_restore_pending = true;
        let active = self.containers.active();
        let wanted = active.selected.iter().chain(active.members.iter());
        let usable = wanted
            .filter_map(|session| self.endpoint_of(session))
            .find(|index| self.endpoints[*index].enabled);
        if let Some(index) = usable
            && index != self.selected_endpoint
        {
            self.show_endpoint(index, cx);
        }
        self.containers_changed(cx);
        cx.notify();
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
            .or_else(|| self.containers.active().members.first().cloned())
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
        // A stopped session never connects by itself: show it parked rather
        // than wait for a connection that will not come.
        if endpoint.enabled
            && (endpoint.is_parked()
                || (endpoint.connection.handle.is_some()
                    && endpoint.live.status.is_connected()
                    && endpoint.live.snapshot.is_some()))
        {
            self.container_restore_pending = false;
            self.show_endpoint(index, cx);
        }
    }

    /// Lists the selected endpoint's session in the shown Window when it is
    /// not there yet, so what is on screen always has a row.
    pub(crate) fn list_selected_session(&mut self, cx: &mut Context<Self>) {
        let session = self.endpoints[self.selected_endpoint].session_ref();
        let shown = self.containers.shown_id();
        if self.containers.active().members.contains(&session) {
            let _ = self.edit_containers(cx, |set| Ok(set.select(shown, &session)));
        } else if self
            .edit_containers(cx, |set| set.attach(shown, session).map(drop))
            .is_err()
        {
            self.local_error = Some("This window lists too many sessions".into());
        }
        self.containers_dirty = true;
    }

    /// Opens the Window this native window showed last time, if it still
    /// exists; the selection inside it waits for its connection.
    pub(crate) fn install_shown(&mut self, shown: Option<ContainerId>) {
        if let Some(id) = shown
            && self.containers.show(id)
        {
            self.container_restore_pending = true;
        }
        self.materialize_slots();
    }
}

#[cfg(test)]
mod tests;
