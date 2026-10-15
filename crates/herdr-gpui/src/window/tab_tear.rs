//! Carrying a tab out of the window to open it in a new one, as a web
//! browser does. Every window lists the same tabs for a workspace, Herdr's
//! and this client's own alike, so the tab is not taken from this window:
//! the new one is another client of the same daemon that goes to the tab's
//! workspace and shows it, while the group here moves on to a neighbour.

use super::{HerdrWindow, tab_drag::TabDrag};
use crate::{
    NavigationTarget,
    app::Placement,
    browser::{Pick, Store},
    window_state::WindowState,
};
use gpui::{Bounds, Context, Window};
use herdr_client::ConnectTarget;

/// A tab carried out of a window, and where the window that shows it opens.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TornTab {
    /// The daemon the new window connects to, as New Window's does.
    pub(crate) target: ConnectTarget,
    /// The host the tab belongs to, which the new window shows.
    pub(super) endpoint: String,
    /// What that host's endpoint was connected to, naming the session it
    /// showed, which the new window's may not.
    pub(super) session: ConnectTarget,
    pub(crate) pick: Pick,
    pub(crate) placement: Placement,
}

/// The tabs torn off in a test, which opens no windows.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TornOff(pub(crate) Vec<TornTab>);

#[cfg(test)]
impl gpui::Global for TornOff {}

impl HerdrWindow {
    /// Opens the tab `drag` carried out of the window in a new window,
    /// placed as if this one had been carried along with it.
    pub(super) fn tear_off_tab(
        &mut self,
        drag: &TabDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let frame = window.bounds();
        let bounds = Bounds::new(frame.origin + (drag.pointer - drag.origin), frame.size);
        let torn = TornTab {
            target: self.endpoints[0].connection.target.clone(),
            endpoint: self.endpoints[self.selected_endpoint].id.clone(),
            session: self.endpoints[self.selected_endpoint]
                .connection
                .target
                .clone(),
            pick: drag.pick.clone(),
            placement: WindowState::beside(bounds, window, cx),
        };
        self.leave_tab(&drag.pick, cx);
        #[cfg(test)]
        cx.default_global::<TornOff>().0.push(torn);
        #[cfg(not(test))]
        crate::app::open_additional_window_then(
            torn.target.clone(),
            Some(torn.placement),
            move |view, window, cx| view.arrive(torn, window, cx),
            cx,
        );
    }

    /// Goes to the tab `torn` names, in a window just opened for it. A page
    /// is chosen against its workspace at once, to show once the window
    /// gets there. The window lists only this machine's daemon until its
    /// saved hosts are read, so the way there waits for the tab's host.
    pub(crate) fn arrive(&mut self, torn: TornTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Pick::Page(id) = torn.pick {
            self.show_browser_tab(id, window, cx);
        }
        self.arrival = Some(torn);
        self.land_arrival(false, cx);
    }

    /// Goes where the waiting torn-off tab is, once its host is listed:
    /// the session it showed there, then the tab, or a page's workspace.
    /// The navigation itself waits for the surface. After the saved hosts
    /// are `listed`, a host still missing is gone, and the tab with it.
    pub(crate) fn land_arrival(&mut self, listed: bool, cx: &mut Context<Self>) {
        let Some(torn) = self.arrival.take() else {
            return;
        };
        if !self
            .endpoints
            .iter()
            .any(|endpoint| endpoint.id == torn.endpoint && endpoint.enabled)
        {
            if !listed {
                self.arrival = Some(torn);
            }
            return;
        }
        if let Some(session) = remote_session(&torn.session) {
            self.attach_session(
                crate::containers::SessionRef::new(torn.endpoint.clone(), session),
                cx,
            );
        }
        let target = match &torn.pick {
            Pick::Herdr(tab) => NavigationTarget::Tab(tab.clone()),
            Pick::Page(id) => {
                let Some(tab) = cx.try_global::<Store>().and_then(|store| store.get(*id)) else {
                    return;
                };
                NavigationTarget::Workspace(tab.workspace_id.clone())
            }
        };
        self.navigate_endpoint(&torn.endpoint, target.as_deref(), cx);
    }
}

/// The session a remote host's endpoint is attached to, which another
/// window of the same host may not be.
pub(super) fn remote_session(target: &ConnectTarget) -> Option<&str> {
    match target {
        ConnectTarget::Ssh { session, .. } | ConnectTarget::Wsl { session, .. } => Some(session),
        _ => None,
    }
}
