//! The registry's JSON form: a versioned envelope of lists, written
//! sorted so the same registry always yields the same bytes. Reading is
//! strict: unknown fields, duplicate or dangling references, values a mutation
//! would refuse, and over-limit counts are errors, never silently repaired.
//!
//! Version 1 holds projects, manual bindings and assignments. Version 2 adds
//! the optional `inferred_paths` list ([`Registry::discover`]'s entries); it
//! is written only when nonempty, so a registry without inferences still
//! serializes exactly as version 1 and a v1 file reads as a v2 one.
use super::{
    DeviceKey, Limit, MAX_ASSIGNMENTS, MAX_BINDINGS, MAX_PROJECTS, Project, ProjectError,
    ProjectId, Registry, RepoKey, SessionRef,
    types::{normalize_path, valid_name},
};
use crate::discovery::valid_session_name;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use std::collections::{BTreeMap, BTreeSet};

const VERSION: u32 = 2;
const V1: u32 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    projects: Vec<ProjectRecord>,
    bindings: Vec<BindingRecord>,
    assignments: Vec<AssignmentRecord>,
    #[serde(default)]
    inferred_paths: Vec<BindingRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectRecord {
    id: ProjectId,
    name: String,
    #[serde(default)]
    repo: Option<RepoKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingRecord {
    device: DeviceKey,
    path: String,
    project: ProjectId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignmentRecord {
    device: DeviceKey,
    session: String,
    project: ProjectId,
}

/// The write-side view of a registry: borrows every field, so encoding or
/// counting never clones a string. Field names and order match [`Envelope`].
struct EnvelopeRef<'a>(&'a Registry);

#[derive(Serialize)]
struct ProjectRef<'a> {
    id: &'a ProjectId,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<&'a RepoKey>,
}

#[derive(Serialize)]
struct BindingRef<'a> {
    device: &'a DeviceKey,
    path: &'a str,
    project: &'a ProjectId,
}

#[derive(Serialize)]
struct AssignmentRef<'a> {
    device: &'a DeviceKey,
    session: &'a str,
    project: &'a ProjectId,
}

impl Serialize for EnvelopeRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let registry = self.0;
        let inferred = !registry.inferred.is_empty();
        let mut out = serializer.serialize_struct("Envelope", if inferred { 5 } else { 4 })?;
        // Version 1 is exactly the older shape, so a registry with no
        // inference writes the bytes older builds read.
        out.serialize_field("version", if inferred { &VERSION } else { &V1 })?;
        out.serialize_field(
            "projects",
            &Seq(|| {
                registry.projects.values().map(|project| ProjectRef {
                    id: &project.id,
                    name: &project.name,
                    repo: project.repo.as_ref(),
                })
            }),
        )?;
        out.serialize_field(
            "bindings",
            &Seq(|| {
                registry
                    .bindings
                    .iter()
                    .map(|((device, path), project)| BindingRef {
                        device,
                        path,
                        project,
                    })
            }),
        )?;
        out.serialize_field(
            "assignments",
            &Seq(|| {
                registry
                    .assignments
                    .iter()
                    .map(|(session, project)| AssignmentRef {
                        device: &session.device,
                        session: &session.session,
                        project,
                    })
            }),
        )?;
        if inferred {
            out.serialize_field(
                "inferred_paths",
                &Seq(|| {
                    registry
                        .inferred
                        .iter()
                        .map(|((device, path), project)| BindingRef {
                            device,
                            path,
                            project,
                        })
                }),
            )?;
        }
        out.end()
    }
}

/// A JSON array produced from an iterator factory.
struct Seq<F>(F);

impl<I, F> Serialize for Seq<F>
where
    F: Fn() -> I,
    I: Iterator,
    I::Item: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq((self.0)())
    }
}

/// Counts bytes and fails once the count passes the limit.
struct LimitedCounter {
    written: usize,
    limit: usize,
}

impl std::io::Write for LimitedCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.written += buf.len();
        if self.written > self.limit {
            return Err(std::io::ErrorKind::FileTooLarge.into());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Registry {
    /// Deterministic JSON (sorted by id, device, then path or session).
    /// Callers write the bytes; this does no file access. Output beyond
    /// [`Registry::MAX_JSON_BYTES`] is refused with [`ProjectError::TooLarge`],
    /// so whatever is written can be read back.
    pub fn to_json(&self) -> Result<Vec<u8>, ProjectError> {
        let bytes = serde_json::to_vec_pretty(&EnvelopeRef(self)).map_err(ProjectError::Schema)?;
        if bytes.len() > Self::MAX_JSON_BYTES {
            return Err(ProjectError::TooLarge);
        }
        Ok(bytes)
    }

    /// Whether [`Registry::to_json`] would succeed, decided without building
    /// the bytes: the encoding is counted as it is produced and abandoned the
    /// moment it passes [`Registry::MAX_JSON_BYTES`]. Saving code calls this
    /// before accepting a change so a state that cannot be written back is
    /// refused up front, not discovered by a background writer.
    pub fn check_saveable(&self) -> Result<(), ProjectError> {
        let mut counter = LimitedCounter {
            written: 0,
            limit: Self::MAX_JSON_BYTES,
        };
        match serde_json::to_writer_pretty(&mut counter, &EnvelopeRef(self)) {
            Ok(()) => Ok(()),
            Err(error) if error.is_io() => Err(ProjectError::TooLarge),
            Err(error) => Err(ProjectError::Schema(error)),
        }
    }

    /// Decode and validate a registry. Input over [`Registry::MAX_JSON_BYTES`]
    /// is refused before parsing.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ProjectError> {
        if bytes.len() > Self::MAX_JSON_BYTES {
            return Err(ProjectError::TooLarge);
        }
        let envelope: Envelope = serde_json::from_slice(bytes).map_err(ProjectError::Schema)?;
        if envelope.version != V1 && envelope.version != VERSION {
            return Err(ProjectError::UnsupportedVersion(envelope.version));
        }
        if envelope.projects.len() > MAX_PROJECTS {
            return Err(ProjectError::LimitReached(Limit::Projects));
        }
        let manual = envelope.bindings.len();
        if manual + envelope.inferred_paths.len() > MAX_BINDINGS {
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
        for record in envelope.inferred_paths {
            if normalize_path(&record.path)? != record.path {
                return Err(ProjectError::InvalidPath);
            }
            if !registry.projects.contains_key(&record.project) {
                return Err(ProjectError::UnknownProject(record.project));
            }
            let key = (record.device, record.path);
            if registry.inferred.insert(key, record.project).is_some() {
                return Err(ProjectError::DuplicateEntry);
            }
        }
        Ok(registry)
    }
}
