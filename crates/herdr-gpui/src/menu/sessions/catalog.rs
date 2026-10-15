//! Search, run-state filter and project grouping over the saved workspace
//! context a session reports. That context comes from the session's persisted
//! `session.json`, so it describes what was saved, not what is live.
use herdr_client::SessionWorkspace;

/// Sessions painted per page. Headers and notes do not count against it.
pub(super) const PAGE_SIZE: usize = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::menu) enum Filter {
    #[default]
    All,
    Running,
    Stopped,
}

impl Filter {
    pub(super) const EACH: [Self; 3] = [Self::All, Self::Running, Self::Stopped];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Running => "Running",
            Self::Stopped => "Stopped",
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            Self::All => Self::Running,
            Self::Running => Self::Stopped,
            Self::Stopped => Self::All,
        }
    }

    pub(super) fn admits(self, running: bool) -> bool {
        match self {
            Self::All => true,
            Self::Running => running,
            Self::Stopped => !running,
        }
    }
}

/// A case-insensitive query: every whitespace-separated word must appear
/// somewhere in the session's device, name, state or saved workspaces.
#[derive(Default)]
pub(super) struct Query {
    words: Vec<String>,
}

impl Query {
    pub(super) fn new(text: &str) -> Self {
        Self {
            words: text.split_whitespace().map(str::to_lowercase).collect(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Index of the workspace satisfying the most query words, so a row shows
    /// why it matched. The first workspace when nothing narrows it.
    pub(super) fn best(&self, workspaces: &[SessionWorkspace]) -> usize {
        let hits = |workspace: &SessionWorkspace| {
            let text = format!(
                "{}\n{}\n{}\n{}",
                workspace.project,
                workspace.path,
                workspace.branch.as_deref().unwrap_or_default(),
                workspace.label.as_deref().unwrap_or_default()
            )
            .to_lowercase();
            self.words
                .iter()
                .filter(|word| text.contains(word.as_str()))
                .count()
        };
        let mut best = (0, 0);
        for (index, workspace) in workspaces.iter().enumerate() {
            let score = hits(workspace);
            if score > best.1 {
                best = (index, score);
            }
        }
        best.0
    }

    pub(super) fn matches(
        &self,
        device: &str,
        session: &str,
        running: bool,
        project_name: Option<&str>,
        workspaces: &[SessionWorkspace],
    ) -> bool {
        if self.words.is_empty() {
            return true;
        }
        let mut haystack = format!(
            "{device}\n{session}\n{}\n{}",
            if running { "running" } else { "stopped" },
            project_name.unwrap_or_default()
        );
        for workspace in workspaces {
            for text in [Some(&workspace.project), Some(&workspace.path)]
                .into_iter()
                .flatten()
                .chain(workspace.branch.as_ref())
                .chain(workspace.label.as_ref())
            {
                haystack.push('\n');
                haystack.push_str(text);
            }
        }
        let haystack = haystack.to_lowercase();
        self.words.iter().all(|word| haystack.contains(word))
    }
}

/// The project a session is grouped under: the first saved workspace that
/// names one. Sessions without any are unclassified, never guessed.
pub(super) fn project(workspaces: &[SessionWorkspace]) -> Option<&str> {
    workspaces
        .iter()
        .map(|workspace| workspace.project.as_str())
        .find(|project| !project.is_empty())
}

/// One line of saved context for a row: the workspace that best matches the
/// query (the first with no query), by full path, with branch and label, and
/// how many more the session saved. It says `saved` because the file can lag
/// the live session.
pub(super) fn summary(workspaces: &[SessionWorkspace], query: &Query) -> Option<String> {
    let shown = workspaces.get(query.best(workspaces))?;
    let place = if shown.path.is_empty() {
        shown.project.as_str()
    } else {
        shown.path.as_str()
    };
    let mut text = format!("saved {place}");
    if let Some(branch) = shown.branch.as_deref().filter(|branch| !branch.is_empty()) {
        text.push_str(&format!(" ({branch})"));
    }
    if let Some(label) = shown.label.as_deref().filter(|label| !label.is_empty()) {
        text.push_str(&format!(" · {label}"));
    }
    if workspaces.len() > 1 {
        text.push_str(&format!(" +{} more", workspaces.len() - 1));
    }
    Some(text)
}

/// Where a session is filed: the project it resolved to, else the first saved
/// workspace's path label, else nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Place {
    Project(String),
    Path(String),
    Nowhere,
}

impl Place {
    /// `project` is the resolved project's name, if any.
    pub(super) fn of(project: Option<&str>, workspaces: &[SessionWorkspace]) -> Self {
        match (project, self::project(workspaces)) {
            (Some(name), _) => Self::Project(name.to_owned()),
            (None, Some(path)) => Self::Path(path.to_owned()),
            (None, None) => Self::Nowhere,
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Project(_) => 0,
            Self::Path(_) => 1,
            Self::Nowhere => 2,
        }
    }

    fn sort_key(&self) -> (u8, String) {
        let text = match self {
            Self::Project(text) | Self::Path(text) => text.to_lowercase(),
            Self::Nowhere => String::new(),
        };
        (self.rank(), text)
    }

    fn heading(&self) -> String {
        match self {
            Self::Project(name) => name.clone(),
            Self::Path(path) => format!("Unassigned · {path}"),
            Self::Nowhere => "Unassigned".to_owned(),
        }
    }
}

/// Reorder `items` so sessions of one project sit together, projects by name,
/// then unassigned sessions (kept apart by their saved path, as before), then
/// those with nothing to file them by, keeping each group's incoming order.
/// Returns the group title preceding each item's first appearance. With no
/// project at all the list stays flat and untitled.
pub(super) fn group<T>(mut items: Vec<T>, place: impl Fn(&T) -> Place) -> Vec<(Option<String>, T)> {
    if items.iter().all(|item| place(item) == Place::Nowhere) {
        return items.into_iter().map(|item| (None, item)).collect();
    }
    items.sort_by_cached_key(|item| place(item).sort_key());
    let mut previous: Option<Place> = None;
    items
        .into_iter()
        .map(|item| {
            let place = place(&item);
            let heading = (previous.as_ref() != Some(&place)).then(|| place.heading());
            previous = Some(place);
            (heading, item)
        })
        .collect()
}
