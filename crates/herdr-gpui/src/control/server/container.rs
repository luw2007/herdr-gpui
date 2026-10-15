//! `container`: the window's logical Windows.
use super::{in_free_window, in_window};
use crate::{
    HerdrWindow,
    containers::SessionRef,
    control::protocol::{
        Call, ErrorCode, Response,
        container::{ContainerInfo, ContainerOp, Reply, SlotInfo, valid_name},
    },
};
use gpui::App;

impl HerdrWindow {
    fn control_containers(&self) -> Vec<ContainerInfo> {
        let active = self.containers.active_index();
        self.containers
            .containers()
            .iter()
            .enumerate()
            .map(|(index, container)| ContainerInfo {
                id: container.id,
                title: container.title.clone(),
                active: index == active,
                sessions: container
                    .slots
                    .iter()
                    .map(|slot| SlotInfo {
                        device: slot.device.clone(),
                        session: slot.session.clone(),
                        selected: container.selected.as_ref() == Some(slot),
                        status: self
                            .endpoint_of(slot)
                            .map_or("not connected", |i| self.endpoints[i].status())
                            .to_owned(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// The device a bare `attach` names: the one the window shows.
    fn shown_device(&self) -> String {
        self.endpoints[self.selected_endpoint]
            .device_id()
            .to_owned()
    }
}

pub(super) fn container(call: Call<ContainerOp>, cx: &mut App) -> Response {
    let caller = &call.caller;
    match call.op {
        ContainerOp::List => in_window(caller, cx, |view, _, _| Response::Container {
            reply: Reply::Containers {
                containers: view.control_containers(),
            },
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
                Ok(index) => Response::done(format!(
                    "Created logical window {}",
                    view.containers.containers()[index].id
                )),
                Err(error) => Response::error(ErrorCode::Busy, error.to_string()),
            }
        }),
        ContainerOp::Focus { id } => in_free_window(caller, cx, |view, _, cx| {
            match view.containers.index_of(id) {
                Some(index) => {
                    view.switch_container(index, cx);
                    Response::done(format!("Showing logical window {id}"))
                }
                None => Response::error(ErrorCode::NotFound, format!("No logical window {id}")),
            }
        }),
        ContainerOp::Attach { session, device } => {
            let named = |name: &str| valid_name(name);
            let valid = named(&session)
                && herdr_client::session_socket(std::path::Path::new(""), &session).is_ok()
                && device.as_deref().is_none_or(named);
            if !valid {
                return Response::error(ErrorCode::InvalidRequest, "Invalid session or device");
            }
            in_free_window(caller, cx, |view, _, cx| {
                let device = device.unwrap_or_else(|| view.shown_device());
                let slot = SessionRef::new(device, session);
                if !view.can_attach(&slot) {
                    return Response::error(
                        ErrorCode::NotFound,
                        format!(
                            "Device {} cannot attach session {}; `host list` shows devices",
                            slot.device, slot.session
                        ),
                    );
                }
                let message = format!(
                    "Attached {} on {} to the active logical window",
                    slot.session, slot.device
                );
                view.attach_session(slot, cx);
                Response::done(message)
            })
        }
    }
}

#[cfg(test)]
mod tests;
