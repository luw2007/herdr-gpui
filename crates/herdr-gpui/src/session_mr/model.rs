//! What a session's pull requests add up to. Lifecycle, draft, checks, review
//! and freshness are separate fields of one [`Pr`]; only [`Bucket`] folds them
//! into the single priority a group is made from. Everything here is pure.

use super::key::Skip;

/// Pull requests named per session. The rest still count toward the bucket.
pub(crate) const NUMBERS_MAX: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    Open,
    Merged,
    Closed,
}

/// What the checks on the head commit add up to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Checks {
    /// None reported, or only skipped ones.
    None,
    Pending,
    Passing,
    Failing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Review {
    /// GitHub reports no decision: no review is required.
    None,
    Required,
    Approved,
    ChangesRequested,
}

/// One pull request as the sessions' state needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pr {
    pub number: u64,
    pub lifecycle: Lifecycle,
    pub draft: bool,
    pub checks: Checks,
    pub review: Review,
}

/// How far an answer can be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Freshness {
    /// The last refresh succeeded.
    Fresh,
    /// The last refresh failed; this is the last successful answer.
    Stale,
}

/// The cached answer for one pull request lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// Never answered: queued or in flight, or failing since the start.
    Unknown,
    /// Wanted, but the cache holds [`KEYS_MAX`](super::cache::KEYS_MAX) keys
    /// already: this one is never asked.
    OverLimit,
    /// Answered. `None` is a successful lookup that found no pull request.
    Known(Option<Pr>, Freshness),
}

/// One workspace of a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceMr {
    /// The session's device or provider is not covered.
    Unsupported,
    /// The workspace cannot be asked about, for this reason.
    Skipped(Skip),
    Looked(Answer),
}

/// The group a session falls in, most urgent first: this declaration order is
/// the order of groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Bucket {
    ChecksFailing,
    ChangesRequested,
    Draft,
    ChecksPending,
    ReviewRequired,
    Approved,
    Open,
    NoPr,
    Merged,
    Closed,
    Unknown,
    /// Not looked up: the Window has more pull request lookups than the cache
    /// holds.
    OverLimit,
    Unsupported,
}

impl Bucket {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::ChecksFailing => "Checks failing",
            Self::ChangesRequested => "Changes requested",
            Self::Draft => "Draft",
            Self::ChecksPending => "Checks pending",
            Self::ReviewRequired => "Review required",
            Self::Approved => "Approved",
            Self::Open => "Open",
            Self::NoPr => "No pull request",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::Unknown => "Unknown",
            Self::OverLimit => "Over lookup limit",
            Self::Unsupported => "Unsupported",
        }
    }

    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::ChecksFailing => "checks-failing",
            Self::ChangesRequested => "changes-requested",
            Self::Draft => "draft",
            Self::ChecksPending => "checks-pending",
            Self::ReviewRequired => "review-required",
            Self::Approved => "approved",
            Self::Open => "open",
            Self::NoPr => "no-pr",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::Unknown => "unknown",
            Self::OverLimit => "over-limit",
            Self::Unsupported => "unsupported",
        }
    }

    /// Whether this bucket is a pull request still in play.
    fn is_open(self) -> bool {
        self < Self::NoPr
    }
}

impl Pr {
    /// The bucket of this one pull request. A failing check outranks a
    /// requested change, which outranks being a draft; lifecycle decides a
    /// merged or closed one before any of that.
    pub(crate) fn bucket(&self) -> Bucket {
        match self.lifecycle {
            Lifecycle::Merged => Bucket::Merged,
            Lifecycle::Closed => Bucket::Closed,
            Lifecycle::Open if self.checks == Checks::Failing => Bucket::ChecksFailing,
            Lifecycle::Open if self.review == Review::ChangesRequested => Bucket::ChangesRequested,
            Lifecycle::Open if self.draft => Bucket::Draft,
            Lifecycle::Open if self.checks == Checks::Pending => Bucket::ChecksPending,
            Lifecycle::Open if self.review == Review::Required => Bucket::ReviewRequired,
            Lifecycle::Open if self.review == Review::Approved => Bucket::Approved,
            Lifecycle::Open => Bucket::Open,
        }
    }
}

impl WorkspaceMr {
    pub(crate) fn bucket(&self) -> Bucket {
        match self {
            Self::Unsupported => Bucket::Unsupported,
            Self::Skipped(_) | Self::Looked(Answer::Unknown) => Bucket::Unknown,
            Self::Looked(Answer::OverLimit) => Bucket::OverLimit,
            Self::Looked(Answer::Known(Some(pr), _)) => pr.bucket(),
            Self::Looked(Answer::Known(None, _)) => Bucket::NoPr,
        }
    }

    fn stale(&self) -> bool {
        matches!(self, Self::Looked(Answer::Known(_, Freshness::Stale)))
    }

    fn number(&self) -> Option<u64> {
        match self {
            Self::Looked(Answer::Known(Some(pr), _)) => Some(pr.number),
            _ => None,
        }
    }
}

/// A session's summary, with every workspace kept beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionMr {
    pub bucket: Bucket,
    /// The workspaces do not agree.
    pub mixed: bool,
    /// Some answer behind the summary is the last good one, not a fresh one.
    pub stale: bool,
    /// Pull requests found, in workspace order, at most [`NUMBERS_MAX`].
    pub numbers: Vec<u64>,
    pub workspaces: Vec<WorkspaceMr>,
}

impl SessionMr {
    /// The summary of a session whose device is not covered at all, with
    /// `workspaces` workspaces behind it (possibly none).
    pub(crate) fn unsupported(workspaces: usize) -> Self {
        Self {
            bucket: Bucket::Unsupported,
            mixed: false,
            stale: false,
            numbers: Vec::new(),
            workspaces: vec![WorkspaceMr::Unsupported; workspaces],
        }
    }

    /// The summary of `workspaces`. The most urgent workspace decides, except
    /// that a workspace whose state is not known (not yet answered, over the
    /// limit, or unsupported) could be open: unless another workspace is a
    /// known open pull request, it keeps the summary from reading merged,
    /// closed, or free of pull requests. Among those, over the limit outranks
    /// not yet known, which outranks unsupported.
    pub(crate) fn of(workspaces: Vec<WorkspaceMr>) -> Self {
        let buckets: Vec<Bucket> = workspaces.iter().map(WorkspaceMr::bucket).collect();
        let best = buckets.iter().copied().min();
        let bucket = match best {
            None => Bucket::Unknown,
            Some(best) if best.is_open() => best,
            Some(Bucket::Unsupported) => Bucket::Unsupported,
            Some(_) if buckets.contains(&Bucket::OverLimit) => Bucket::OverLimit,
            Some(_) if buckets.contains(&Bucket::Unknown) => Bucket::Unknown,
            Some(_) if buckets.contains(&Bucket::Unsupported) => Bucket::Unsupported,
            Some(best) => best,
        };
        let mixed = buckets.windows(2).any(|pair| pair[0] != pair[1]);
        Self {
            bucket,
            mixed,
            stale: workspaces.iter().any(WorkspaceMr::stale),
            numbers: workspaces
                .iter()
                .filter_map(WorkspaceMr::number)
                .take(NUMBERS_MAX)
                .collect(),
            workspaces,
        }
    }
}
