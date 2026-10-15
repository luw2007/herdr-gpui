//! `project`: the app's logical folders across devices.
use super::{Args, CliError, unknown};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectCommand {
    List,
    /// A task project, or a git project when `repo` names its repository.
    Create {
        name: String,
        repo: Option<String>,
    },
    Rename {
        project: String,
        name: String,
    },
    Bind {
        project: String,
        path: String,
        device: Option<String>,
    },
    Unbind {
        path: String,
        device: Option<String>,
    },
    Assign {
        project: String,
        session: Option<String>,
        device: Option<String>,
    },
    Unassign {
        session: Option<String>,
        device: Option<String>,
    },
    Show {
        session: Option<String>,
        device: Option<String>,
    },
    /// Opens the app's confirmation; never deletes by itself.
    Delete {
        project: String,
    },
}

pub(super) fn project(args: &mut Args) -> Result<ProjectCommand, CliError> {
    let Some(verb) = args.verb() else {
        return Ok(ProjectCommand::List);
    };
    let command = match verb.to_str() {
        Some("list") => ProjectCommand::List,
        Some("create") => {
            let repo = args.text_option("--repo")?;
            let task = args.flag("--task");
            if repo.is_some() && task {
                return Err(CliError::InvalidValue("--repo with --task"));
            }
            ProjectCommand::Create {
                name: name(args)?,
                repo,
            }
        }
        Some("rename") => ProjectCommand::Rename {
            project: args.required_text("PROJECT")?,
            name: name(args)?,
        },
        Some("bind") => {
            let device = args.text_option("--device")?;
            ProjectCommand::Bind {
                project: args.required_text("PROJECT")?,
                path: args.required_text("PATH")?,
                device,
            }
        }
        Some("unbind") => {
            let device = args.text_option("--device")?;
            ProjectCommand::Unbind {
                path: args.required_text("PATH")?,
                device,
            }
        }
        Some("assign") => {
            let (session, device) = target(args)?;
            ProjectCommand::Assign {
                project: args.required_text("PROJECT")?,
                session,
                device,
            }
        }
        Some("unassign") => {
            let (session, device) = target(args)?;
            ProjectCommand::Unassign { session, device }
        }
        Some("show") => {
            let (session, device) = target(args)?;
            ProjectCommand::Show { session, device }
        }
        Some("delete") => ProjectCommand::Delete {
            project: args.required_text("PROJECT")?,
        },
        _ => return Err(unknown("project", verb)),
    };
    args.finish()?;
    Ok(command)
}

/// A project name: the words after the project, joined as typed unquoted.
fn name(args: &mut Args) -> Result<String, CliError> {
    args.words()?.ok_or(CliError::MissingArgument {
        command: args.name(),
        what: "NAME",
    })
}

/// `--session NAME` and `--device ID`.
fn target(args: &mut Args) -> Result<(Option<String>, Option<String>), CliError> {
    Ok((
        args.text_option("--session")?,
        args.text_option("--device")?,
    ))
}
