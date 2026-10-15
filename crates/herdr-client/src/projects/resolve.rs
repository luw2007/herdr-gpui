//! Which project a session belongs to.
//!
//! Order: (1) an explicit assignment; else the saved workspaces in order, the
//! first that resolves wins, and within one workspace (2) the longest manual
//! binding on the session's device that covers the workspace path or its
//! repository root, then (3) the project of the workspace's repository
//! remote. Nothing else (name, label, branch) is ever consulted.
//!
//! A binding covers a path equal to it or below it. Paths compare by
//! components: `/` and `\` both separate, empty segments are ignored, and each
//! segment must match exactly (case-sensitive, `.`/`..` unresolved), so
//! `/work/app` covers `/work/app/src` but not `/work/app2`. The repository
//! root is checked too so a linked worktree kept outside the repository
//! directory still resolves through its repository.
use super::{DeviceKey, ProjectId, Registry, RepoKey, SessionRef, types::components};
use crate::SessionWorkspace;

/// Where a resolution came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The session was assigned to the project explicitly.
    Assigned,
    /// A manual binding, given as stored, covers the workspace.
    Path(String),
    /// The workspace's repository is the project's repository.
    Repository(RepoKey),
}

/// A session's project, if any.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    pub project: Option<ProjectId>,
    pub source: Option<Source>,
}

impl Registry {
    /// The project of `session` given its saved `workspaces`.
    pub fn resolve(&self, session: &SessionRef, workspaces: &[SessionWorkspace]) -> Resolution {
        if let Some(project) = self.assignments.get(session) {
            return Resolution {
                project: Some(project.clone()),
                source: Some(Source::Assigned),
            };
        }
        workspaces
            .iter()
            .find_map(|workspace| self.resolve_workspace(&session.device, workspace))
            .unwrap_or_default()
    }

    fn resolve_workspace(
        &self,
        device: &DeviceKey,
        workspace: &SessionWorkspace,
    ) -> Option<Resolution> {
        if let Some((path, project)) = self.longest_binding(device, workspace) {
            return Some(Resolution {
                project: Some(project.clone()),
                source: Some(Source::Path(path.to_owned())),
            });
        }
        let repo = workspace.repo.as_ref()?;
        let project = self.project_for_repo(repo)?;
        Some(Resolution {
            project: Some(project.id.clone()),
            source: Some(Source::Repository(repo.clone())),
        })
    }

    /// The deepest binding of `device` covering the workspace path or root.
    /// Equal depth keeps the earlier candidate (path before root, then stored
    /// order), so the answer is deterministic.
    fn longest_binding(
        &self,
        device: &DeviceKey,
        workspace: &SessionWorkspace,
    ) -> Option<(&str, &ProjectId)> {
        let targets = [components(&workspace.path), components(&workspace.project)];
        let mut best: Option<(usize, &str, &ProjectId)> = None;
        let start = (device.clone(), String::new());
        for ((bound_device, bound), project) in self.bindings.range(start..) {
            if bound_device != device {
                break;
            }
            let prefix = components(bound);
            if targets.iter().any(|target| target.starts_with(&prefix))
                && best.is_none_or(|(depth, ..)| prefix.len() > depth)
            {
                best = Some((prefix.len(), bound, project));
            }
        }
        best.map(|(_, path, project)| (path, project))
    }
}
