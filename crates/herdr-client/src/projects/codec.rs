//! The registry's JSON form: a `version: 1` envelope of three lists, written
//! sorted so the same registry always yields the same bytes. Reading is
//! strict: unknown fields, duplicate or dangling references, values a mutation
//! would refuse, and over-limit counts are errors, never silently repaired.
use super::{
    DeviceKey, Limit, MAX_ASSIGNMENTS, MAX_BINDINGS, MAX_PROJECTS, Project, ProjectError,
    ProjectId, Registry, RepoKey, SessionRef,
    types::{normalize_path, valid_name},
};
use crate::discovery::valid_session_name;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    projects: Vec<ProjectRecord>,
    bindings: Vec<BindingRecord>,
    assignments: Vec<AssignmentRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectRecord {
    id: ProjectId,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repo: Option<RepoKey>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingRecord {
    device: DeviceKey,
    path: String,
    project: ProjectId,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignmentRecord {
    device: DeviceKey,
    session: String,
    project: ProjectId,
}

impl Registry {
    /// Deterministic JSON (sorted by id, device, then path or session).
    /// Callers write the bytes; this does no file access. Output beyond
    /// [`Registry::MAX_JSON_BYTES`] is refused with [`ProjectError::TooLarge`],
    /// so whatever is written can be read back.
    pub fn to_json(&self) -> Result<Vec<u8>, ProjectError> {
        let envelope = Envelope {
            version: VERSION,
            projects: self
                .projects
                .values()
                .map(|project| ProjectRecord {
                    id: project.id.clone(),
                    name: project.name.clone(),
                    repo: project.repo.clone(),
                })
                .collect(),
            bindings: self
                .bindings
                .iter()
                .map(|((device, path), project)| BindingRecord {
                    device: device.clone(),
                    path: path.clone(),
                    project: project.clone(),
                })
                .collect(),
            assignments: self
                .assignments
                .iter()
                .map(|(session, project)| AssignmentRecord {
                    device: session.device.clone(),
                    session: session.session.clone(),
                    project: project.clone(),
                })
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&envelope).map_err(ProjectError::Schema)?;
        if bytes.len() > Self::MAX_JSON_BYTES {
            return Err(ProjectError::TooLarge);
        }
        Ok(bytes)
    }

    /// Decode and validate a registry. Input over [`Registry::MAX_JSON_BYTES`]
    /// is refused before parsing.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProjectError> {
        if bytes.len() > Self::MAX_JSON_BYTES {
            return Err(ProjectError::TooLarge);
        }
        let envelope: Envelope = serde_json::from_slice(bytes).map_err(ProjectError::Schema)?;
        if envelope.version != VERSION {
            return Err(ProjectError::UnsupportedVersion(envelope.version));
        }
        if envelope.projects.len() > MAX_PROJECTS {
            return Err(ProjectError::LimitReached(Limit::Projects));
        }
        if envelope.bindings.len() > MAX_BINDINGS {
            return Err(ProjectError::LimitReached(Limit::Bindings));
        }
        if envelope.assignments.len() > MAX_ASSIGNMENTS {
            return Err(ProjectError::LimitReached(Limit::Assignments));
        }
        let mut registry = Self::new();
        let mut names = BTreeSet::new();
        let mut repos = BTreeMap::new();
        for record in envelope.projects {
            if valid_name(&record.name)? != record.name {
                return Err(ProjectError::InvalidName);
            }
            if !names.insert(record.name.to_lowercase()) {
                return Err(ProjectError::DuplicateName(record.name));
            }
            if let Some(repo) = &record.repo
                && let Some(existing) = repos.insert(repo.clone(), record.id.clone())
            {
                return Err(ProjectError::DuplicateRepo { existing });
            }
            let project = Project {
                id: record.id,
                name: record.name,
                repo: record.repo,
            };
            if registry
                .projects
                .insert(project.id.clone(), project)
                .is_some()
            {
                return Err(ProjectError::DuplicateEntry);
            }
        }
        for record in envelope.bindings {
            if normalize_path(&record.path)? != record.path {
                return Err(ProjectError::InvalidPath);
            }
            if !registry.projects.contains_key(&record.project) {
                return Err(ProjectError::UnknownProject(record.project));
            }
            let key = (record.device, record.path);
            if registry.bindings.insert(key, record.project).is_some() {
                return Err(ProjectError::DuplicateEntry);
            }
        }
        for record in envelope.assignments {
            if !valid_session_name(&record.session) {
                return Err(ProjectError::InvalidSession);
            }
            if !registry.projects.contains_key(&record.project) {
                return Err(ProjectError::UnknownProject(record.project));
            }
            let session = SessionRef::new(record.device, record.session);
            if registry
                .assignments
                .insert(session, record.project)
                .is_some()
            {
                return Err(ProjectError::DuplicateEntry);
            }
        }
        Ok(registry)
    }
}
