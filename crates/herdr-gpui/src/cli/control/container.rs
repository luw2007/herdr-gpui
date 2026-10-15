//! `container`: the logical Windows inside one native window.
use super::{Args, CliError, unknown};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerCommand {
    List,
    New {
        title: Option<String>,
    },
    Focus {
        id: u64,
    },
    Attach {
        session: String,
        device: Option<String>,
    },
}

pub(super) fn container(args: &mut Args) -> Result<ContainerCommand, CliError> {
    let Some(verb) = args.verb() else {
        return Ok(ContainerCommand::List);
    };
    Ok(match verb.to_str() {
        Some("list") => ContainerCommand::List,
        Some("new") => ContainerCommand::New {
            title: args.text_option("--title")?,
        },
        Some("focus") => ContainerCommand::Focus {
            id: args
                .required_text("ID")?
                .parse()
                .map_err(|_| CliError::InvalidValue("ID"))?,
        },
        Some("attach") => ContainerCommand::Attach {
            device: args.text_option("--device")?,
            session: args.required_text("SESSION")?,
        },
        _ => return Err(unknown("container", verb)),
    })
}
