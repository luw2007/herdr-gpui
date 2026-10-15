//! The in-memory registry and its mutation API. Every mutation validates first
//! and changes nothing on failure, so a registry is always internally
//! consistent: no duplicate names or repositories, no binding or assignment
//! naming a missing project.
use super::{
    Binding, DeviceKey, Limit, MAX_ASSIGNMENTS, MAX_BINDINGS, MAX_PROJECTS, Project, ProjectError,
    ProjectId, RepoKey, SessionRef,
    types::{normalize_path, valid_name},
};
use crate::discovery::valid_session_name;
use std::collections::BTreeMap;

/// Projects, per-device path bindings, and explicit session assignments.
/// Collections are ordered maps so iteration and serialization are stable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registry {
    pub(super) projects: BTreeMap<ProjectId, Project>,
    /// `(device, normalized path)` to the project that folder belongs to.
    pub(super) bindings: BTreeMap<(DeviceKey, String), ProjectId>,
    pub(super) assignments: BTreeMap<SessionRef, ProjectId>,
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl Registry {
    /// The largest registry file [`Registry::from_json`] accepts. Callers must
    /// not read more than this many bytes into memory before decoding.
    pub const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.projects.is_empty() && self.bindings.is_empty() && self.assignments.is_empty()
    }

    /// Make a project. `repo: Some` makes a git project, joined to every
    /// device's clone through the remote; `None` makes a task-code project that
    /// only manual bindings reach. The name is trimmed.
    pub fn create_project(
        &mut self,
        name: &str,
        repo: Option<RepoKey>,
    ) -> Result<Project, ProjectError> {
        let name = valid_name(name)?;
        self.check_name(&name, None)?;
        if let Some(repo) = &repo
            && let Some(existing) = self.project_for_repo(repo)
        {
            return Err(ProjectError::DuplicateRepo {
                existing: existing.id.clone(),
            });
        }
        if self.projects.len() >= MAX_PROJECTS {
            return Err(ProjectError::LimitReached(Limit::Projects));
        }
        let project = Project {
            id: ProjectId::generate(),
            name,
            repo,
        };
        self.projects.insert(project.id.clone(), project.clone());
        Ok(project)
    }

    pub fn rename_project(&mut self, id: &ProjectId, name: &str) -> Result<(), ProjectError> {
        let name = valid_name(name)?;
        if !self.projects.contains_key(id) {
            return Err(ProjectError::UnknownProject(id.clone()));
        }
        self.check_name(&name, Some(id))?;
        if let Some(project) = self.projects.get_mut(id) {
            project.name = name;
        }
        Ok(())
    }

    /// Delete a project together with its bindings and the assignments that
    /// pointed at it; those sessions become unassigned (or are inferred again).
    pub fn remove_project(&mut self, id: &ProjectId) -> Result<Project, ProjectError> {
        let project = self
            .projects
            .remove(id)
            .ok_or_else(|| ProjectError::UnknownProject(id.clone()))?;
        self.bindings.retain(|_, bound| bound != id);
        self.assignments.retain(|_, assigned| assigned != id);
        Ok(project)
    }

    /// Bind a folder on a device to a project. Binding the same folder to the
    /// same project again changes nothing; to another project it conflicts
    /// (unbind first).
    pub fn bind(
        &mut self,
        device: DeviceKey,
        path: &str,
        project: &ProjectId,
    ) -> Result<Binding, ProjectError> {
        let path = normalize_path(path)?;
        if !self.projects.contains_key(project) {
            return Err(ProjectError::UnknownProject(project.clone()));
        }
        let key = (device, path);
        match self.bindings.get(&key) {
            Some(existing) if existing != project => {
                return Err(ProjectError::BindingConflict {
                    device: key.0,
                    existing: existing.clone(),
                });
            }
            Some(_) => {}
            None => {
                if self.bindings.len() >= MAX_BINDINGS {
                    return Err(ProjectError::LimitReached(Limit::Bindings));
                }
                self.bindings.insert(key.clone(), project.clone());
            }
        }
        Ok(Binding {
            device: key.0,
            path: key.1,
            project: project.clone(),
        })
    }

    /// Remove the binding of a folder; whether there was one.
    pub fn unbind(&mut self, device: &DeviceKey, path: &str) -> Result<bool, ProjectError> {
        let path = normalize_path(path)?;
        Ok(self.bindings.remove(&(device.clone(), path)).is_some())
    }

    /// Put a session in a project, replacing any earlier assignment.
    pub fn assign(&mut self, session: SessionRef, project: &ProjectId) -> Result<(), ProjectError> {
        if !valid_session_name(&session.session) {
            return Err(ProjectError::InvalidSession);
        }
        if !self.projects.contains_key(project) {
            return Err(ProjectError::UnknownProject(project.clone()));
        }
        if !self.assignments.contains_key(&session) && self.assignments.len() >= MAX_ASSIGNMENTS {
            return Err(ProjectError::LimitReached(Limit::Assignments));
        }
        self.assignments.insert(session, project.clone());
        Ok(())
    }

    /// Drop a session's explicit assignment; whether there was one. The session
    /// may still be inferred into a project from its workspaces.
    pub fn unassign(&mut self, session: &SessionRef) -> bool {
        self.assignments.remove(session).is_some()
    }

    pub fn project(&self, id: &ProjectId) -> Option<&Project> {
        self.projects.get(id)
    }

    pub fn project_for_repo(&self, repo: &RepoKey) -> Option<&Project> {
        self.projects
            .values()
            .find(|project| project.repo.as_ref() == Some(repo))
    }

    /// Projects for display: by name (case-insensitive), then id.
    pub fn projects(&self) -> Vec<&Project> {
        let mut projects: Vec<&Project> = self.projects.values().collect();
        projects.sort_by_cached_key(|project| (project.name.to_lowercase(), project.id.clone()));
        projects
    }

    /// A project's bindings, ordered by device then path.
    pub fn bindings_of(&self, project: &ProjectId) -> Vec<Binding> {
        self.bindings
            .iter()
            .filter(|(_, bound)| *bound == project)
            .map(|((device, path), project)| Binding {
                device: device.clone(),
                path: path.clone(),
                project: project.clone(),
            })
            .collect()
    }

    /// The project a session was explicitly assigned to.
    pub fn assignment(&self, session: &SessionRef) -> Option<&ProjectId> {
        self.assignments.get(session)
    }

    /// Sessions explicitly assigned to a project, ordered by device then name.
    pub fn assignments_of(&self, project: &ProjectId) -> Vec<&SessionRef> {
        self.assignments
            .iter()
            .filter(|(_, assigned)| *assigned == project)
            .map(|(session, _)| session)
            .collect()
    }

    fn check_name(&self, name: &str, except: Option<&ProjectId>) -> Result<(), ProjectError> {
        if self
            .projects
            .values()
            .any(|project| Some(&project.id) != except && same_name(&project.name, name))
        {
            return Err(ProjectError::DuplicateName(name.to_owned()));
        }
        Ok(())
    }
}
