//! `container`: the app's Windows.
use super::{Args, CliError, unknown};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerCommand {
    List,
    New {
        title: Option<String>,
    },
    Focus {
        window: String,
    },
    Rename {
        window: String,
        title: String,
    },
    Close {
        window: String,
    },
    Move {
        window: String,
        position: usize,
    },
    MoveSession {
        session: String,
        device: Option<String>,
        window: Option<String>,
        position: usize,
    },
    Group {
        window: String,
        by: String,
    },
    Attach {
        session: String,
        device: Option<String>,
        window: Option<String>,
    },
    Detach {
        session: String,
        device: Option<String>,
        window: Option<String>,
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
            window: args.required_text("WINDOW")?,
        },
        Some("rename") => ContainerCommand::Rename {
            window: args.required_text("WINDOW")?,
            title: args.required_text("TITLE")?,
        },
        Some("close") => ContainerCommand::Close {
            window: args.required_text("WINDOW")?,
        },
        Some("move") => ContainerCommand::Move {
            window: args.required_text("WINDOW")?,
            position: args
                .required_text("POSITION")?
                .parse()
                .map_err(|_| CliError::InvalidValue("POSITION"))?,
        },
        Some("move-session") => ContainerCommand::MoveSession {
            device: args.text_option("--device")?,
            window: args.text_option("--in")?,
            session: args.required_text("SESSION")?,
            position: args
                .required_text("POSITION")?
                .parse()
                .map_err(|_| CliError::InvalidValue("POSITION"))?,
        },
        Some("group") => ContainerCommand::Group {
            window: args.required_text("WINDOW")?,
            by: args.required_text("BY")?,
        },
        Some("attach") => ContainerCommand::Attach {
            device: args.text_option("--device")?,
            window: args.text_option("--in")?,
            session: args.required_text("SESSION")?,
        },
        Some("detach") => ContainerCommand::Detach {
            device: args.text_option("--device")?,
            window: args.text_option("--in")?,
            session: args.required_text("SESSION")?,
        },
        _ => return Err(unknown("container", verb)),
    })
}
