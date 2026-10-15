//! `project`: the app's projects, logical folders across devices. Every
//! request is answered at once on the UI thread from the shared store; the
//! store's mutations queue their save on a worker, so nothing here waits on
//! the disk. A session or device is named exactly: the caller's own window
//! decides what "current" means, and a caller whose workspace no window shows
//! is refused rather than answered from another session's window.
//!
//! `project delete` never deletes: it opens the Sessions manager on its own
//! delete-project confirmation and the user confirms there.
use super::{in_free_window, in_window, work::flows::bring_forward};
use crate::{
    HerdrWindow,
    control::protocol::{
        Call, Caller, ErrorCode, Response,
        projects::{
            BindingInfo, LIST_BUDGET, MAX_LISTED_BINDINGS, MAX_LISTED_PROJECTS, ProjectInfo,
            ProjectOp, ProjectRef, Reply, Resolved, SourceInfo, parse_repo,
        },
    },
    projects::Projects,
};
use gpui::App;
use herdr_client::{
    ConnectTarget, DeviceKey, Project, ProjectError, ProjectId, Registry, SessionRef,
    SessionWorkspace, Source,
};

#[cfg(test)]
mod tests;

/// How many candidates an ambiguity names.
const MAX_CANDIDATES: usize = 5;

pub(super) fn project(call: Call<ProjectOp>, cx: &mut App) -> Response {
    let Call { caller, op } = call;
    if let Err(message) = op.validate() {
        return Response::error(ErrorCode::InvalidRequest, message);
    }
    match op {
        ProjectOp::Delete { project } => in_free_window(&caller, cx, |view, window, cx| {
            let found = match Projects::read(cx, |registry| find_project(registry, &project)) {
                Ok(found) => found,
                Err(response) => return response,
            };
            if !view.confirm_project_removal(found.id.clone(), window, cx) {
                return Response::error(
                    ErrorCode::Failed,
                    "The Sessions manager did not open; close other menus and try again",
                );
            }
            bring_forward(window, cx);
            Response::Project {
                reply: Reply::DeletePending {
                    project: project_ref(&found),
                },
            }
        }),
        op => in_window(&caller, cx, |view, _, cx| run(op, view, &caller, cx)),
    }
}

fn run(op: ProjectOp, view: &HerdrWindow, caller: &Caller, cx: &mut App) -> Response {
    match op {
        ProjectOp::List => list(cx),
        ProjectOp::Create { name, repo } => {
            let repo = repo.as_deref().and_then(parse_repo);
            match Projects::update(cx, |store| store.create_project(&name, repo)) {
                Ok(created) => Response::Project {
                    reply: Reply::Project {
                        project: ProjectInfo {
                            id: created.id.to_string(),
                            name: created.name,
                            repo: created.repo.map(|repo| repo.to_string()),
                            bindings: Vec::new(),
                            bindings_total: 0,
                            sessions: 0,
                        },
                    },
                },
                Err(error) => refuse(&error),
            }
        }
        ProjectOp::Rename { project, name } => {
            let found = match Projects::read(cx, |registry| find_project(registry, &project)) {
                Ok(found) => found,
                Err(response) => return response,
            };
            match Projects::update(cx, |store| store.rename_project(&found.id, &name)) {
                Ok(()) => Response::done(format!("Renamed {} to {}", found.name, name.trim())),
                Err(error) => refuse(&error),
            }
        }
        ProjectOp::Bind {
            project,
            path,
            device,
        } => {
            let device = match device_of(view, caller, device.as_deref(), false) {
                Ok(device) => device,
                Err(response) => return response,
            };
            let found = match Projects::read(cx, |registry| find_project(registry, &project)) {
                Ok(found) => found,
                Err(response) => return response,
            };
            match Projects::update(cx, |store| store.bind(&device, &path, &found.id)) {
                Ok(binding) => Response::done(format!(
                    "Bound {} on {} to {}",
                    binding.path, binding.device, found.name
                )),
                Err(error) => refuse(&error),
            }
        }
        ProjectOp::Unbind { path, device } => {
            let device = match device_of(view, caller, device.as_deref(), true) {
                Ok(device) => device,
                Err(response) => return response,
            };
            match Projects::update(cx, |store| store.unbind(&device, &path)) {
                Ok(true) => Response::done(format!("Unbound {path} on {device}")),
                Ok(false) => Response::error(
                    ErrorCode::NotFound,
                    format!("No project is bound to {path} on {device}"),
                ),
                Err(error) => refuse(&error),
            }
        }
        ProjectOp::Assign {
            project,
            session,
            device,
        } => {
            let subject = match subject(view, caller, session.as_deref(), device.as_deref()) {
                Ok(subject) => subject,
                Err(response) => return response,
            };
            let found = match Projects::read(cx, |registry| find_project(registry, &project)) {
                Ok(found) => found,
                Err(response) => return response,
            };
            match Projects::update(cx, |store| {
                store.assign(&subject.device, &subject.session, &found.id)
            }) {
                Ok(()) => Response::done(format!(
                    "Assigned session {} on {} to {}",
                    subject.session, subject.device, found.name
                )),
                Err(error) => refuse(&error),
            }
        }
        ProjectOp::Unassign { session, device } => {
            let subject = match subject(view, caller, session.as_deref(), device.as_deref()) {
                Ok(subject) => subject,
                Err(response) => return response,
            };
            match Projects::update(cx, |store| {
                store.unassign(&subject.device, &subject.session)
            }) {
                Ok(true) => Response::done(format!(
                    "Unassigned session {} on {}",
                    subject.session, subject.device
                )),
                Ok(false) => Response::done(format!(
                    "Session {} on {} had no explicit assignment; a project found from its folder or repository stays",
                    subject.session, subject.device
                )),
                Err(error) => refuse(&error),
            }
        }
        ProjectOp::Show { session, device } => {
            match subject(view, caller, session.as_deref(), device.as_deref()) {
                Ok(subject) => show(view, &subject, cx),
                Err(response) => response,
            }
        }
        // Handled before a window is chosen.
        ProjectOp::Delete { .. } => Response::error(ErrorCode::Failed, "Unexpected request"),
    }
}

/// A store failure as the code a caller can act on.
fn refuse(error: &crate::Error) -> Response {
    let code = match error {
        crate::Error::Project(error) => match error {
            ProjectError::UnknownProject(_) => ErrorCode::NotFound,
            ProjectError::InvalidName
            | ProjectError::InvalidId
            | ProjectError::InvalidDevice
            | ProjectError::InvalidSession
            | ProjectError::InvalidPath => ErrorCode::InvalidRequest,
            _ => ErrorCode::Failed,
        },
        _ => ErrorCode::Failed,
    };
    Response::error(code, error.to_string())
}

fn project_ref(project: &Project) -> ProjectRef {
    ProjectRef {
        id: project.id.to_string(),
        name: project.name.clone(),
        repo: project.repo.as_ref().map(ToString::to_string),
    }
}

fn info(registry: &Registry, project: &Project) -> ProjectInfo {
    let bindings = registry.bindings_of(&project.id);
    ProjectInfo {
        id: project.id.to_string(),
        name: project.name.clone(),
        repo: project.repo.as_ref().map(ToString::to_string),
        bindings_total: bindings.len(),
        bindings: bindings
            .into_iter()
            .take(MAX_LISTED_BINDINGS)
            .map(|binding| BindingInfo {
                device: binding.device.to_string(),
                path: binding.path,
            })
            .collect(),
        sessions: registry.assignments_of(&project.id).len(),
    }
}

/// Projects by name, as many as fit one bounded answer.
fn list(cx: &App) -> Response {
    Projects::read(cx, |registry| {
        let all = registry.projects();
        let mut projects = Vec::new();
        let mut size = 0;
        for project in all.iter().take(MAX_LISTED_PROJECTS) {
            let item = info(registry, project);
            size += 96
                + item.name.len()
                + item.repo.as_ref().map_or(0, String::len)
                + item
                    .bindings
                    .iter()
                    .map(|binding| 24 + binding.device.len() + binding.path.len())
                    .sum::<usize>();
            if size > LIST_BUDGET {
                break;
            }
            projects.push(item);
        }
        Response::Project {
            reply: Reply::Projects {
                projects,
                total: all.len(),
            },
        }
    })
}

/// The project a reference names: an id, else one exact name compared
/// without regard to case. A name two projects share is never guessed.
fn find_project(registry: &Registry, reference: &str) -> Result<Project, Response> {
    if let Ok(id) = ProjectId::parse(reference)
        && let Some(project) = registry.project(&id)
    {
        return Ok(project.clone());
    }
    let matches: Vec<&Project> = registry
        .projects()
        .into_iter()
        .filter(|project| project.name.eq_ignore_ascii_case(reference))
        .collect();
    match matches.as_slice() {
        [] => Err(Response::error(
            ErrorCode::NotFound,
            format!("No project is named {reference}; `herdr-gpui project list` lists them"),
        )),
        [only] => Ok((*only).clone()),
        many => Err(ambiguous(
            "project",
            reference,
            many.len(),
            many.iter().map(|project| project.id.to_string()),
            "use its id",
        )),
    }
}

fn ambiguous(
    what: &str,
    name: &str,
    count: usize,
    ids: impl Iterator<Item = String>,
    hint: &str,
) -> Response {
    let ids: Vec<String> = ids.take(MAX_CANDIDATES).collect();
    let more = if count > ids.len() { ", ..." } else { "" };
    Response::error(
        ErrorCode::Ambiguous,
        format!(
            "{name} names {count} {what}s ({}{more}); {hint}",
            ids.join(", ")
        ),
    )
}

/// The session a request is about: a device by its endpoint id, and a
/// session by its name on that device.
struct Subject {
    device: String,
    session: String,
}

/// Matches `name` against the window's devices by endpoint id, else by one
/// label compared without regard to case.
fn endpoint_named(view: &HerdrWindow, name: &str) -> Result<usize, Response> {
    if let Some(index) = view.endpoints.iter().position(|e| e.id == name) {
        return Ok(index);
    }
    let labelled: Vec<usize> = view
        .endpoints
        .iter()
        .enumerate()
        .filter(|(_, endpoint)| endpoint.label.eq_ignore_ascii_case(name))
        .map(|(index, _)| index)
        .collect();
    match labelled.as_slice() {
        [index] => Ok(*index),
        [] => Err(Response::error(
            ErrorCode::NotFound,
            format!("No device is named {name}; `herdr-gpui host list` lists them"),
        )),
        many => Err(ambiguous(
            "device",
            name,
            many.len(),
            many.iter().map(|index| view.endpoints[*index].id.clone()),
            "use its id",
        )),
    }
}

/// The device the caller's request is about. Without a name it is the
/// device showing the caller's workspace, else the window's selected one; a
/// caller whose workspace the window does not show is refused, as the window
/// belongs to another session. `allow_unknown` lets a name that matches no
/// device through as an endpoint id, so a binding left by a removed device
/// can still be dropped.
fn device_of(
    view: &HerdrWindow,
    caller: &Caller,
    named: Option<&str>,
    allow_unknown: bool,
) -> Result<String, Response> {
    match named {
        Some(name) => match endpoint_named(view, name) {
            Ok(index) => Ok(view.endpoints[index].id.clone()),
            Err(response) if allow_unknown && DeviceKey::new(name).is_ok() => match response {
                Response::Error {
                    code: ErrorCode::NotFound,
                    ..
                } => Ok(name.to_owned()),
                other => Err(other),
            },
            Err(response) => Err(response),
        },
        None => current_index(view, caller).map(|index| view.endpoints[index].id.clone()),
    }
}

fn current_index(view: &HerdrWindow, caller: &Caller) -> Result<usize, Response> {
    if caller.workspace_id.is_some() || caller.daemon_socket.is_some() {
        return super::hosts::Place::of(view, caller).map(|place| place.index);
    }
    Ok(view
        .selected_endpoint
        .min(view.endpoints.len().saturating_sub(1)))
}

/// The session the window's device is attached to, when its name is known.
fn attached_session(target: &ConnectTarget) -> Option<String> {
    match target {
        ConnectTarget::Session { name, .. } => Some(name.clone()),
        ConnectTarget::Ssh { session, .. } | ConnectTarget::Wsl { session, .. } => {
            Some(session.clone())
        }
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud { session, .. } => Some(session.clone()),
        // The default session unless the environment names another or a
        // socket; the socket does not say which session it is.
        ConnectTarget::Local => {
            let override_set = ["HERDR_SOCKET_PATH", "HERDR_CLIENT_SOCKET_PATH"]
                .iter()
                .any(|name| std::env::var_os(name).is_some());
            if override_set {
                return None;
            }
            match std::env::var("HERDR_SESSION") {
                Ok(name) if crate::control::protocol::projects::valid_session(&name) => Some(name),
                Ok(_) => None,
                Err(_) => Some("default".to_owned()),
            }
        }
        ConnectTarget::Socket(_) => None,
    }
}

fn subject(
    view: &HerdrWindow,
    caller: &Caller,
    session: Option<&str>,
    device: Option<&str>,
) -> Result<Subject, Response> {
    let index = match device {
        Some(name) => endpoint_named(view, name)?,
        None => current_index(view, caller)?,
    };
    let endpoint = &view.endpoints[index];
    let session = match session {
        Some(name) => name.to_owned(),
        None => attached_session(&endpoint.connection.target).ok_or_else(|| {
            Response::error(
                ErrorCode::InvalidRequest,
                format!(
                    "Cannot tell which session {} is attached to; name it with --session",
                    endpoint.label
                ),
            )
        })?,
    };
    Ok(Subject {
        device: endpoint.id.clone(),
        session,
    })
}

/// The project a session belongs to and what decided it. The saved
/// workspaces come from the Sessions manager's last scan; this never starts
/// one, so a session no scan has seen resolves only by assignment.
fn show(view: &HerdrWindow, subject: &Subject, cx: &App) -> Response {
    let workspaces = view.saved_workspaces(&subject.device, &subject.session);
    let known: &[SessionWorkspace] = workspaces.as_deref().unwrap_or_default();
    let Ok(device) = DeviceKey::new(subject.device.clone()) else {
        return Response::error(
            ErrorCode::InvalidRequest,
            ProjectError::InvalidDevice.to_string(),
        );
    };
    let session = SessionRef::new(device, subject.session.clone());
    let (project, source) = Projects::read(cx, |registry| {
        let resolution = registry.resolve(&session, known);
        let project = resolution
            .project
            .and_then(|id| registry.project(&id).map(project_ref));
        (project, resolution.source)
    });
    let source = source.map(|source| match source {
        Source::Assigned => SourceInfo::Assigned,
        Source::Path(path) => SourceInfo::Path { path },
        Source::Repository(repo) => SourceInfo::Repository {
            repo: repo.to_string(),
        },
    });
    let note = match (&project, &workspaces) {
        (Some(_), _) => None,
        (None, None) => Some(
            "No project is assigned, and the session's saved folders have not been scanned; open the Sessions manager to scan them"
                .to_owned(),
        ),
        (None, Some(_)) => Some(
            "No project is assigned, and no folder or repository of the session belongs to one"
                .to_owned(),
        ),
    };
    Response::Project {
        reply: Reply::Resolved(Box::new(Resolved {
            device: subject.device.clone(),
            session: subject.session.clone(),
            project,
            source,
            note,
        })),
    }
}
