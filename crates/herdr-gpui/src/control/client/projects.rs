//! `project`: print what the app keeps about projects, or ask it to change.
mod text;

#[cfg(test)]
mod manage;

#[cfg(test)]
mod tests;

use super::{Context, Output, unexpected};
use crate::{
    cli::control::ProjectCommand,
    control::protocol::{
        Request, Response,
        projects::{ProjectOp, Reply},
    },
};

pub(super) fn run(command: ProjectCommand, context: &Context) -> crate::Result<Output> {
    match context.call(Request::Project, operation(command))? {
        Response::Done { message } => Ok(Output::message(message)),
        Response::Project { reply } => output(&reply),
        response => Err(unexpected(response)),
    }
}

fn operation(command: ProjectCommand) -> ProjectOp {
    match command {
        ProjectCommand::List => ProjectOp::List,
        ProjectCommand::Manage => ProjectOp::Manage {},
        ProjectCommand::Create { name, repo } => ProjectOp::Create { name, repo },
        ProjectCommand::Rename { project, name } => ProjectOp::Rename { project, name },
        ProjectCommand::Bind {
            project,
            path,
            device,
        } => ProjectOp::Bind {
            project,
            path,
            device,
        },
        ProjectCommand::Unbind { path, device } => ProjectOp::Unbind { path, device },
        ProjectCommand::Assign {
            project,
            session,
            device,
        } => ProjectOp::Assign {
            project,
            session,
            device,
        },
        ProjectCommand::Unassign { session, device } => ProjectOp::Unassign { session, device },
        ProjectCommand::Show { session, device } => ProjectOp::Show { session, device },
        ProjectCommand::Delete { project } => ProjectOp::Delete { project },
    }
}

fn output(reply: &Reply) -> crate::Result<Output> {
    match reply {
        Reply::Projects { projects, total } => {
            let output = Output::data(text::list(projects, *total), reply)?;
            Ok(if projects.is_empty() {
                output.nothing()
            } else {
                output
            })
        }
        Reply::Project { project } => Output::data(text::created(project), reply),
        Reply::Resolved(resolved) => Output::data(text::resolved(resolved), reply),
        Reply::DeletePending { project } => Output::data(text::delete_pending(project), reply),
    }
}
