//! The window's side of grouping: the cached session context, the folded
//! groups, and the sidebar's host order under the shown Window's grouping.
//!
//! Folded groups are window-local and not persisted: they key on the Window
//! and the group, so a project or status that comes back is folded again, and
//! they are forgotten when the Window is closed.

use super::{
    GroupKey, Member, ProjectTag, Status, project,
    scan::{Request, Scan},
};
use crate::{HerdrWindow, containers::ContainerId, state::ConnectionStatus};
use gpui::Context;
use herdr_client::{DeviceKey, protocol::AgentStatus};
use std::{collections::HashSet, time::Instant};

#[derive(Default)]
pub(crate) struct State {
    pub(super) scan: Scan,
    pub(super) mr: crate::session_mr::Mr,
    folded: HashSet<(ContainerId, GroupKey)>,
    /// Whether this window has taken the projects registry yet. The registry
    /// mirror otherwise only follows changes, so a window opened onto saved
    /// projects reads it once, on its first poll.
    synced: bool,
}

#[cfg(test)]
impl State {
    pub(crate) fn mr_busy(&self) -> bool {
        self.mr.busy()
    }

    pub(crate) fn scan_mut(&mut self) -> &mut Scan {
        &mut self.scan
    }
}

/// A headed run of hosts as the sidebar paints it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Section {
    pub key: GroupKey,
    pub title: String,
    pub folded: bool,
    /// Endpoint indices, in the Window's member order.
    pub hosts: Vec<usize>,
}

impl HerdrWindow {
    /// What an endpoint is doing, for status grouping.
    pub(crate) fn endpoint_status(&self, index: usize) -> Status {
        let endpoint = &self.endpoints[index];
        let live = if index == self.selected_endpoint {
            &self.live
        } else {
            &endpoint.live
        };
        if !endpoint.enabled || live.status == ConnectionStatus::Detached {
            return Status::Stopped;
        }
        if !live.status.is_connected() {
            return Status::Disconnected;
        }
        let agents = live
            .snapshot
            .as_deref()
            .map_or(&[][..], |snapshot| snapshot.agents.as_slice());
        if agents
            .iter()
            .any(|agent| agent.agent_status == AgentStatus::Blocked)
        {
            Status::Waiting
        } else if agents
            .iter()
            .any(|agent| agent.agent_status == AgentStatus::Working)
        {
            Status::Working
        } else {
            Status::Idle
        }
    }

    /// The sections the sidebar paints for the shown Window, or `None` when it
    /// lists hosts flat. `keep` narrows the hosts, as the Devices layout's
    /// search does; the device filter already applies through `endpoint_listed`.
    pub(crate) fn sidebar_sections(&self, keep: impl Fn(usize) -> bool) -> Option<Vec<Section>> {
        let window = self.containers.active();
        if window.grouping == crate::containers::Grouping::None {
            return None;
        }
        let shown = self.containers.shown_id();
        let mut listed: Vec<(usize, usize)> = (0..self.endpoints.len())
            .filter(|&index| self.endpoint_listed(index) && keep(index))
            .map(|index| {
                let session = self.endpoints[index].session_ref();
                let position = window
                    .members
                    .iter()
                    .position(|member| *member == session)
                    .unwrap_or(usize::MAX);
                (position, index)
            })
            .collect();
        // Window order; a stable sort keeps endpoint order among equals.
        listed.sort_by_key(|&(position, _)| position);
        let members: Vec<Member> = listed
            .into_iter()
            .map(|(_, index)| Member {
                item: index,
                project: self.project_of(index),
                status: self.endpoint_status(index),
                mr: self.mr_bucket(index),
            })
            .collect();
        let groups = project(window.grouping, &members)?;
        Some(
            groups
                .into_iter()
                .map(|group| Section {
                    folded: self.grouping.folded.contains(&(shown, group.key.clone())),
                    key: group.key,
                    title: group.title,
                    hosts: group.items,
                })
                .collect(),
        )
    }

    /// The project of the session behind endpoint `index`: an explicit
    /// assignment, else what its cached saved workspaces resolve to.
    fn project_of(&self, index: usize) -> Option<ProjectTag> {
        let session = self.endpoints[index].session_ref();
        let registry = &self.sessions.projects;
        let id = registry
            .resolve(&session, self.grouping.scan.workspaces(&session))
            .project?;
        let name = registry.project(&id)?.name.clone();
        Some(ProjectTag { id, name })
    }

    /// Every listed host in the order the sidebar paints it, folded or not.
    pub(crate) fn sidebar_host_order(&self) -> Vec<usize> {
        match self.sidebar_sections(|_| true) {
            Some(sections) => sections.into_iter().flat_map(|s| s.hosts).collect(),
            None => (0..self.endpoints.len())
                .filter(|&index| self.endpoint_listed(index))
                .collect(),
        }
    }

    /// The hosts a folded group hides.
    pub(crate) fn folded_hosts(&self) -> HashSet<usize> {
        self.sidebar_sections(|_| true)
            .into_iter()
            .flatten()
            .filter(|section| section.folded)
            .flat_map(|section| section.hosts)
            .collect()
    }

    /// Folds the shown Window's group, or opens it again.
    pub(crate) fn toggle_group(&mut self, key: GroupKey, cx: &mut Context<Self>) {
        let entry = (self.containers.shown_id(), key);
        if !self.grouping.folded.remove(&entry) {
            self.grouping.folded.insert(entry);
        }
        cx.notify();
    }

    /// Keeps the session context project grouping reads current. Cheap while
    /// grouping is off or by status; otherwise it starts at most one worker at
    /// a time, and the render only ever reads what the worker delivered.
    pub(crate) fn poll_grouping(&mut self, cx: &mut Context<Self>) {
        let windows = self.containers.set();
        self.grouping
            .folded
            .retain(|(id, _)| windows.get(*id).is_some());
        let grouping = self.containers.active().grouping;
        // The lookup runs only while the shown Window groups by MR.
        if grouping != crate::containers::Grouping::Mr {
            self.grouping.mr.stop();
        }
        if !matches!(
            grouping,
            crate::containers::Grouping::Project | crate::containers::Grouping::Mr
        ) {
            // Neither dimension reads the scan: abandon an SSH round in flight.
            self.grouping.scan.stop();
            return;
        }
        let mut changed = false;
        if grouping == crate::containers::Grouping::Project && !self.grouping.synced {
            self.grouping.synced = true;
            self.sync_projects(cx);
            changed = true;
        }
        let request = self.grouping_request();
        changed |= self.grouping.scan.poll(&request, Instant::now());
        if grouping == crate::containers::Grouping::Mr {
            let held = cx
                .try_global::<crate::pull_request::Shared>()
                .map(|shared| shared.pauses(Instant::now()))
                .unwrap_or_default();
            self.grouping.mr.hold(held);
            changed |= self.poll_mr();
        }
        if changed {
            cx.notify();
        }
    }

    /// The shown Window's sessions and the hosts that can list their devices.
    fn grouping_request(&self) -> Request {
        let sessions = self.containers.active().members.clone();
        let hosts: Vec<(DeviceKey, herdr_client::RemoteHost)> = self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.slot.is_none() && endpoint.enabled)
            .filter_map(|endpoint| {
                let host = endpoint.connection.target.remote_host()?;
                let device = DeviceKey::new(endpoint.id.clone()).ok()?;
                sessions
                    .iter()
                    .any(|session| session.device == device)
                    .then_some((device, host))
            })
            .collect();
        Request::new(sessions, self.local_development(), hosts)
    }
}
