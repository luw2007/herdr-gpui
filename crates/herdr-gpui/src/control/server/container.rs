//! `container`: the app's global Windows, as seen from one native window.
use super::{in_free_window, in_window};
use crate::{
    HerdrWindow,
    containers::{ContainerError, ContainerId, ContainerSet, Grouping},
    control::protocol::{
        Call, ErrorCode, Response,
        container::{ContainerInfo, ContainerOp, LIST_BUDGET, Reply, SlotInfo, valid_name},
    },
};
use gpui::App;
use herdr_client::{DeviceKey, SessionRef};

impl HerdrWindow {
    fn control_containers(&self) -> Vec<ContainerInfo> {
        let shown = self.containers.shown_id();
        let mut all: Vec<ContainerInfo> = self
            .containers
            .containers()
            .iter()
            .map(|container| ContainerInfo {
                id: container.id.to_string(),
                title: container.title.clone(),
                grouping: container.grouping.as_str().to_owned(),
                active: container.id == shown,
                sessions: container
                    .members
                    .iter()
                    .map(|member| SlotInfo {
                        device: member.device.to_string(),
                        session: member.session.clone(),
                        selected: container.selected.as_ref() == Some(member),
                        status: self
                            .endpoint_of(member)
                            .map_or("not connected", |i| self.endpoints[i].status())
                            .to_owned(),
                    })
                    .collect(),
                sessions_total: container.members.len(),
            })
            .collect();
        fit_budget(&mut all, LIST_BUDGET);
        all
    }

    /// The device a bare `attach` names: the one the window shows.
    fn shown_device(&self) -> String {
        self.endpoints[self.selected_endpoint]
            .device_id()
            .to_owned()
    }
}

/// Cuts each Window's sessions so the whole list serializes within `budget`
/// bytes. Sizes are measured, not estimated. A list that already fits is left
/// whole. Otherwise the budget is shared out fairly: a Window needing less than
/// an equal share keeps everything and hands the rest on, so a full Window
/// cannot hide the sessions of the others and small Windows never waste what
/// they do not use. `sessions_total` says what was left out.
fn fit_budget(containers: &mut [ContainerInfo], budget: usize) {
    // Per Window: its own fields plus the comma that follows it, and each
    // session plus its comma.
    let measured: Vec<(usize, Vec<usize>)> = containers
        .iter()
        .map(|container| {
            let head = serde_json::to_vec(&ContainerInfo {
                sessions: Vec::new(),
                ..container.clone()
            })
            .map_or(0, |bytes| bytes.len())
                + 1;
            let slots = container
                .sessions
                .iter()
                .map(|slot| serde_json::to_vec(slot).map_or(usize::MAX, |bytes| bytes.len() + 1))
                .collect();
            (head, slots)
        })
        .collect();
    let demand = |(head, slots): &(usize, Vec<usize>)| {
        slots
            .iter()
            .fold(*head, |sum, size| sum.saturating_add(*size))
    };
    if measured
        .iter()
        .fold(0usize, |sum, window| sum.saturating_add(demand(window)))
        <= budget
    {
        return;
    }
    // Smallest demand first, so what a small Window leaves unused is shared by
    // those that follow.
    let mut order: Vec<usize> = (0..containers.len()).collect();
    order.sort_by_key(|&index| demand(&measured[index]));
    let mut left = budget;
    let count = order.len();
    for (rank, index) in order.into_iter().enumerate() {
        let (head, slots) = &measured[index];
        let allowance = demand(&measured[index]).min(left / (count - rank));
        let room = allowance.saturating_sub(*head);
        let mut used = 0usize;
        let mut fits = 0;
        for size in slots {
            if used.saturating_add(*size) > room {
                break;
            }
            used += size;
            fits += 1;
        }
        // Only what was really kept is spent; the rest goes to the others.
        left = left.saturating_sub(head + used);
        containers[index].sessions.truncate(fits);
    }
}
/// The Window `reference` names: an ID, else an exact name.
fn find(set: &ContainerSet, reference: &str) -> Result<ContainerId, Response> {
    let by_id = ContainerId::parse(reference).filter(|id| set.get(*id).is_some());
    by_id.or_else(|| set.by_name(reference)).ok_or_else(|| {
        Response::error(
            ErrorCode::NotFound,
            format!("No window {reference}; `container list` shows them"),
        )
    })
}

fn refused(error: &ContainerError) -> Response {
    let code = match error {
        ContainerError::InvalidTitle => ErrorCode::InvalidRequest,
        ContainerError::DuplicateTitle(_)
        | ContainerError::NotEmpty
        | ContainerError::TooManyContainers(_)
        | ContainerError::TooManySlots(_) => ErrorCode::Failed,
        ContainerError::NoSuchContainer | ContainerError::NoSuchMember => ErrorCode::NotFound,
        ContainerError::InvalidState => ErrorCode::InvalidRequest,
    };
    Response::error(code, error.to_string())
}

/// The session a request names, or the response refusing it.
fn session_of(
    view: &HerdrWindow,
    session: String,
    device: Option<String>,
) -> Result<SessionRef, Response> {
    let device = device.unwrap_or_else(|| view.shown_device());
    DeviceKey::new(device)
        .map(|device| SessionRef::new(device, session))
        .map_err(|_| Response::error(ErrorCode::InvalidRequest, "Invalid session or device"))
}

pub(super) fn container(call: Call<ContainerOp>, cx: &mut App) -> Response {
    let caller = &call.caller;
    match call.op {
        ContainerOp::List => in_window(caller, cx, |view, _, cx| {
            // Edits made by other callers a moment ago are in the registry.
            view.sync_containers(cx);
            Response::Container {
                reply: Reply::Containers {
                    containers: view.control_containers(),
                },
            }
        }),
        ContainerOp::New { title } => in_free_window(caller, cx, |view, _, cx| {
            if let Some(title) = &title
                && crate::containers::clean_title(title).is_err()
            {
                return Response::error(
                    ErrorCode::InvalidRequest,
                    "Title must be 1-64 printable characters",
                );
            }
            match view.new_container(title.as_deref(), cx) {
                Ok(id) => Response::done(format!("Created window {id}")),
                Err(error) => refused(&error),
            }
        }),
        ContainerOp::Focus { window } => in_free_window(caller, cx, |view, _, cx| {
            view.sync_containers(cx);
            let id = match find(view.containers.set(), &window) {
                Ok(id) => id,
                Err(response) => return response,
            };
            if view.switch_container(id, cx) {
                Response::done(format!("Showing window {id}"))
            } else {
                Response::error(ErrorCode::NotFound, format!("No window {window}"))
            }
        }),
        ContainerOp::Rename { window, title } => in_free_window(caller, cx, |view, _, cx| {
            view.sync_containers(cx);
            match find(view.containers.set(), &window) {
                Ok(id) => match view.rename_container(id, &title, cx) {
                    Ok(()) => Response::done(format!("Renamed window {id}")),
                    Err(error) => refused(&error),
                },
                Err(response) => response,
            }
        }),
        ContainerOp::Close { window } => in_free_window(caller, cx, |view, _, cx| {
            view.sync_containers(cx);
            let id = match find(view.containers.set(), &window) {
                Ok(id) => id,
                Err(response) => return response,
            };
            match view.close_container(id, cx) {
                Ok(()) => Response::done(format!("Closed window {id}")),
                Err(error) => refused(&error),
            }
        }),
        ContainerOp::Move { window, position } => in_free_window(caller, cx, |view, _, cx| {
            view.sync_containers(cx);
            let id = match find(view.containers.set(), &window) {
                Ok(id) => id,
                Err(response) => return response,
            };
            let to = position.saturating_sub(1);
            match view.edit_containers(cx, |set| set.move_container(id, to)) {
                Ok(()) => {
                    cx.notify();
                    Response::done(format!("Moved window {id} to position {position}"))
                }
                Err(error) => refused(&error),
            }
        }),
        ContainerOp::Group { window, by } => in_free_window(caller, cx, |view, _, cx| {
            view.sync_containers(cx);
            let Some(grouping) = Grouping::parse(&by) else {
                return Response::error(
                    ErrorCode::InvalidRequest,
                    "Group by none, project, status or mr",
                );
            };
            let id = match find(view.containers.set(), &window) {
                Ok(id) => id,
                Err(response) => return response,
            };
            match view.edit_containers(cx, |set| set.set_grouping(id, grouping)) {
                Ok(()) => {
                    cx.notify();
                    Response::done(format!("Window {id} groups by {by}"))
                }
                Err(error) => refused(&error),
            }
        }),
        ContainerOp::MoveSession {
            session,
            device,
            window,
            position,
        } => {
            let named = |name: &str| valid_name(name);
            if !named(&session) || !device.as_deref().is_none_or(named) {
                return Response::error(ErrorCode::InvalidRequest, "Invalid session or device");
            }
            in_free_window(caller, cx, |view, _, cx| {
                view.sync_containers(cx);
                let session = match session_of(view, session, device) {
                    Ok(session) => session,
                    Err(response) => return response,
                };
                let target = match window.as_deref() {
                    Some(reference) => match find(view.containers.set(), reference) {
                        Ok(id) => id,
                        Err(response) => return response,
                    },
                    None => view.containers.shown_id(),
                };
                let to = position.saturating_sub(1);
                match view.edit_containers(cx, |set| set.move_member(target, &session, to)) {
                    Ok(()) => {
                        cx.notify();
                        Response::done(format!("Moved {} to position {position}", session.session))
                    }
                    Err(error) => refused(&error),
                }
            })
        }
        ContainerOp::Attach {
            session,
            device,
            window,
        } => {
            let named = |name: &str| valid_name(name);
            let valid = named(&session)
                && herdr_client::session_socket(std::path::Path::new(""), &session).is_ok()
                && device.as_deref().is_none_or(named)
                && window.as_deref().is_none_or(named);
            if !valid {
                return Response::error(ErrorCode::InvalidRequest, "Invalid session or device");
            }
            in_free_window(caller, cx, |view, _, cx| {
                view.sync_containers(cx);
                let session = match session_of(view, session, device) {
                    Ok(session) => session,
                    Err(response) => return response,
                };
                let target = match window.as_deref() {
                    Some(reference) => match find(view.containers.set(), reference) {
                        Ok(id) => id,
                        Err(response) => return response,
                    },
                    None => view.containers.shown_id(),
                };
                if !view.can_attach(&session) {
                    return Response::error(
                        ErrorCode::NotFound,
                        format!(
                            "Device {} cannot attach session {}; `host list` shows devices",
                            session.device, session.session
                        ),
                    );
                }
                let message = format!(
                    "Attached {} on {} to window {target}",
                    session.session, session.device
                );
                let attached = if target == view.containers.shown_id() {
                    view.attach_shown(session, cx)
                } else {
                    view.attach_session_to(target, session, cx)
                };
                if let Err(error) = attached {
                    return Response::error(ErrorCode::Failed, error.to_string());
                }
                Response::done(message)
            })
        }
        ContainerOp::Detach {
            session,
            device,
            window,
        } => {
            let named = |name: &str| valid_name(name);
            if !named(&session) || !device.as_deref().is_none_or(named) {
                return Response::error(ErrorCode::InvalidRequest, "Invalid session or device");
            }
            in_free_window(caller, cx, |view, _, cx| {
                view.sync_containers(cx);
                let session = match session_of(view, session, device) {
                    Ok(session) => session,
                    Err(response) => return response,
                };
                let target = match window.as_deref() {
                    Some(reference) => match find(view.containers.set(), reference) {
                        Ok(id) => id,
                        Err(response) => return response,
                    },
                    None => view.containers.shown_id(),
                };
                if view.detach_session_from(target, &session, cx) {
                    Response::done(format!(
                        "Detached {} on {} from window {target}",
                        session.session, session.device
                    ))
                } else {
                    Response::error(
                        ErrorCode::NotFound,
                        format!(
                            "Window {target} does not list {} on {}",
                            session.session, session.device
                        ),
                    )
                }
            })
        }
    }
}

#[cfg(test)]
mod tests;
