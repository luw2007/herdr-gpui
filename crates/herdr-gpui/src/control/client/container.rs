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
        ContainerCommand::Focus { id } => ContainerOp::Focus { id },
        ContainerCommand::Attach { session, device } => ContainerOp::Attach { session, device },
    };
    match context.call(Request::Container, op)? {
        Response::Done { message } => Ok(Output::message(message)),
        Response::Container {
            reply: Reply::Containers { containers },
        } => {
            let mut text = String::new();
            for container in &containers {
                let _ = writeln!(
                    text,
                    "{}{} {}",
                    if container.active { "*" } else { " " },
                    container.id,
                    container.title
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
            }
            Output::data(text.trim_end(), &containers)
        }
        response => Err(unexpected(response)),
    }
}
