//! What a confirmed, successful session deletion leaves behind in the app's
//! own registries. It is keyed by the session captured at confirmation, never
//! by a live endpoint: a session no Window attached has none.
use super::*;
use crate::projects::Projects;
use herdr_client::SessionRef;

impl HerdrWindow {
    /// The session `row` names, as the registries identify it.
    pub(super) fn deleted_session_ref(&self, row: &Row) -> Option<SessionRef> {
        let (device, session) = projects::identity(&self.endpoints[0].id, row)?;
        Some(session_ref(&device, session))
    }

    /// Forgets a deleted session: its paused mark and Window members (which
    /// move to the device's default, as for an attached one) and its project
    /// assignment. Called only once the deletion succeeded; a failure or a
    /// timeout, whose outcome is unknown, leaves everything as it was.
    pub(super) fn forget_deleted_session(&mut self, session: &SessionRef, cx: &mut Context<Self>) {
        if session.session != "default" {
            self.container_rewrites.push((
                session.device.clone(),
                session.session.clone(),
                "default".to_owned(),
            ));
        }
        self.containers_changed(cx);
        // The registry cannot fail for a key it already holds; a refused change
        // leaves it as it was.
        let _ = Projects::update(cx, |store| {
            store.unassign(session.device.as_str(), &session.session)
        });
        self.sync_projects(cx);
    }
}
