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
        workspaces: &[SessionWorkspace],
    ) -> bool {
        if self.words.is_empty() {
            return true;
        }
        let mut haystack = format!(
            "{device}\n{session}\n{}",
            if running { "running" } else { "stopped" }
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

/// Reorder `items` so sessions of one project sit together, projects by name
/// and unclassified last, keeping each group's incoming order. Returns the
/// group title preceding each item's first appearance.
pub(super) fn group<T>(
    mut items: Vec<T>,
    workspaces: impl Fn(&T) -> &[SessionWorkspace],
) -> Vec<(Option<String>, T)> {
    let classified = items.iter().any(|item| project(workspaces(item)).is_some());
    if !classified {
        return items.into_iter().map(|item| (None, item)).collect();
    }
    items.sort_by(|a, b| {
        let key = |item: &T| project(workspaces(item)).map(str::to_lowercase);
        match (key(a), key(b)) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    let mut previous: Option<Option<String>> = None;
    items
        .into_iter()
        .map(|item| {
            let title = project(workspaces(&item)).map(str::to_owned);
            let heading = (previous.as_ref() != Some(&title))
                .then(|| title.clone().unwrap_or_else(|| "Unclassified".to_owned()));
            previous = Some(title);
            (heading, item)
        })
        .collect()
}
