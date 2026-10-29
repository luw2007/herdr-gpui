//! `container`.
use super::{Context, Output, unexpected};
use crate::{
    cli::control::ContainerCommand,
    control::protocol::{
        Request, Response,
        container::{ContainerOp, Reply},
    },
};
use std::fmt::Write as _;

pub(super) fn container(command: ContainerCommand, context: &Context) -> crate::Result<Output> {
    let op = match command {
        ContainerCommand::List => ContainerOp::List,
        ContainerCommand::New { title } => ContainerOp::New { title },
        ContainerCommand::Focus { window } => ContainerOp::Focus { window },
        ContainerCommand::Rename { window, title } => ContainerOp::Rename { window, title },
        ContainerCommand::Close { window } => ContainerOp::Close { window },
        ContainerCommand::Move { window, position } => ContainerOp::Move { window, position },
        ContainerCommand::MoveSession {
            session,
            device,
            window,
            position,
        } => ContainerOp::MoveSession {
            session,
            device,
            window,
            position,
        },
        ContainerCommand::Group { window, by } => ContainerOp::Group { window, by },
        ContainerCommand::Attach {
            session,
            device,
            window,
        } => ContainerOp::Attach {
            session,
            device,
            window,
        },
        ContainerCommand::Detach {
            session,
            device,
            window,
        } => ContainerOp::Detach {
            session,
            device,
            window,
        },
    };
    match context.call(Request::Container, op)? {
        Response::Done { message } => Ok(Output::message(message)),
        Response::Container { reply } => output(&reply),
        response => Err(unexpected(response)),
    }
}

fn output(reply: &Reply) -> crate::Result<Output> {
    match reply {
        Reply::Containers { containers } => {
            let mut text = String::new();
            for container in containers {
                let _ = writeln!(
                    text,
                    "{}{} {} ({})",
                    if container.active { "*" } else { " " },
                    container.id,
                    container.title,
                    container.grouping
                );
                for slot in &container.sessions {
                    let _ = writeln!(
                        text,
                        "    {} {} on {}: {}",
                        if slot.selected { ">" } else { " " },
                        slot.session,
                        slot.device,
                        slot.status
                    );
                }
                if container.sessions_total > container.sessions.len() {
                    let _ = writeln!(
                        text,
                        "    ... {} more not listed; the answer is cut to fit",
                        container.sessions_total - container.sessions.len()
                    );
                }
            }
            Output::data(text.trim_end(), containers)
        }
        Reply::ClosePending { window } => Output::data(
            format!(
                "Asked to close {} ({}); confirm in Herdr UI. Nothing is closed yet",
                window.title, window.id
            ),
            reply,
        ),
    }
}

#[cfg(test)]
mod tests;
