//! How project answers read in a terminal. Pure, so the layout is tested
//! without an app; `finish` makes the app's text safe to print.
use crate::control::protocol::projects::{ProjectInfo, ProjectRef, Resolved, SourceInfo};
use std::fmt::Write as _;

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn kind(repo: Option<&str>) -> String {
    repo.map_or_else(|| "task project".to_owned(), |repo| format!("repo {repo}"))
}

pub(super) fn list(projects: &[ProjectInfo], total: usize) -> String {
    if projects.is_empty() {
        return if total == 0 {
            "No projects yet; `herdr-gpui project create NAME` makes one".into()
        } else {
            format!(
                "{} but none fit in one answer",
                plural(total, "project", "projects")
            )
        };
    }
    let mut text = String::new();
    for project in projects {
        let _ = writeln!(
            text,
            "{}  {}  [{}]  {}",
            project.id,
            project.name,
            kind(project.repo.as_deref()),
            plural(project.sessions, "assigned session", "assigned sessions")
        );
        for binding in &project.bindings {
            let _ = writeln!(text, "    {}: {}", binding.device, binding.path);
        }
        if project.bindings_total > project.bindings.len() {
            let _ = writeln!(
                text,
                "    ... and {} more",
                project.bindings_total - project.bindings.len()
            );
        }
    }
    if total > projects.len() {
        let _ = writeln!(text, "... and {} more projects", total - projects.len());
    }
    text.truncate(text.trim_end().len());
    text
}

pub(super) fn created(project: &ProjectInfo) -> String {
    format!(
        "Created {} ({}) [{}]",
        project.name,
        project.id,
        kind(project.repo.as_deref())
    )
}

pub(super) fn resolved(resolved: &Resolved) -> String {
    let session = format!("Session {} on {}", resolved.session, resolved.device);
    let Some(project) = &resolved.project else {
        return match &resolved.note {
            Some(note) => format!("{session}: no project. {note}"),
            None => format!("{session}: no project"),
        };
    };
    let why = match &resolved.source {
        Some(SourceInfo::Assigned) => "assigned".to_owned(),
        Some(SourceInfo::Path { path }) => format!("folder binding {path}"),
        Some(SourceInfo::Repository { repo }) => format!("repository {repo}"),
        None => "unknown".to_owned(),
    };
    format!("{session}: {} ({}) via {why}", project.name, project.id)
}

pub(super) fn delete_pending(project: &ProjectRef) -> String {
    format!(
        "Asked to delete {} ({}); confirm in the Sessions manager in Herdr UI. Nothing is deleted yet",
        project.name, project.id
    )
}
