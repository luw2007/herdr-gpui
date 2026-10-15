use super::*;
use crate::SessionWorkspace;

mod codec;
mod registry;
mod repo_key;
mod resolve;

fn device(key: &str) -> DeviceKey {
    DeviceKey::new(key).unwrap()
}

fn session(device_key: &str, name: &str) -> SessionRef {
    SessionRef::new(device(device_key), name)
}

fn key(url: &str) -> RepoKey {
    RepoKey::from_remote_url(url).unwrap()
}

fn workspace(project: &str, path: &str, repo: Option<&str>) -> SessionWorkspace {
    SessionWorkspace {
        project: project.into(),
        path: path.into(),
        branch: None,
        label: None,
        repo: repo.map(key),
    }
}
