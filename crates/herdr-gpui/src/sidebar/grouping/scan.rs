//! The saved workspaces of the Window's sessions, which project grouping
//! resolves projects from. Reading them is I/O (`session.json` on this
//! machine, one SSH or WSL round trip per remote device), so it runs on one
//! worker thread and never on the render path: render reads the cache only.
//!
//! The scan only reads. It never starts a session; the local listing probes
//! sockets and reads saved state, and a remote device answers its own CLI's
//! listing, exactly as the Sessions picker does.
//!
//! The cache is invalidated by age ([`INTERVAL`]), and at once when the request
//! stops being covered by the last one: a session joined the Window, a device
//! changed its host, or the development flag flipped.

use herdr_client::{
    DeviceKey, LocalSession, RemoteHost, RemoteSession, SessionRef, SessionWorkspace,
    list_local_sessions,
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// How long an answer stands. A session's saved workspaces change when the
/// user opens a workspace in it, and every remote answer costs a connection.
pub(super) const INTERVAL: Duration = Duration::from_secs(30);

/// What a scan is asked for: the Window's sessions and the way to reach the
/// devices they live on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Request {
    /// Sorted, without repeats.
    pub sessions: Vec<SessionRef>,
    pub development: bool,
    /// Remote devices among the sessions' devices, sorted by device.
    pub hosts: Vec<(DeviceKey, RemoteHost)>,
}

impl Request {
    pub(crate) fn new(
        mut sessions: Vec<SessionRef>,
        development: bool,
        mut hosts: Vec<(DeviceKey, RemoteHost)>,
    ) -> Self {
        sessions.sort();
        sessions.dedup();
        hosts.sort_by(|a, b| a.0.cmp(&b.0));
        hosts.dedup_by(|a, b| a.0 == b.0);
        Self {
            sessions,
            development,
            hosts,
        }
    }

    /// Whether an answer to `self` also answers `other`.
    fn covers(&self, other: &Self) -> bool {
        self.development == other.development
            && self.hosts == other.hosts
            && other
                .sessions
                .iter()
                .all(|session| self.sessions.binary_search(session).is_ok())
    }

    fn wants_local(&self) -> bool {
        let local = crate::endpoint::LOCAL;
        self.sessions
            .iter()
            .any(|session| session.device.as_str() == local)
    }
}

/// Why a device could not be listed. The cause stays typed; only the log line
/// that reports it turns it into text.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DeviceError {
    #[error("could not list this machine's sessions")]
    Local(#[source] herdr_client::Error),
    #[error("could not list the device's sessions")]
    Remote(#[source] herdr_client::Error),
    #[error("the device probe stopped unexpectedly")]
    Stopped,
    /// The scan was told to stop before it asked this device.
    #[error("the session scan was cancelled")]
    Cancelled,
}

/// One device's sessions with their saved workspaces, or why the device could
/// not be listed.
pub(crate) type DeviceAnswer = (
    DeviceKey,
    Result<Vec<(String, Vec<SessionWorkspace>)>, DeviceError>,
);

#[derive(Default)]
pub(crate) struct Scan {
    known: HashMap<SessionRef, Vec<SessionWorkspace>>,
    asked: Option<Request>,
    pending: Option<mpsc::Receiver<Vec<DeviceAnswer>>>,
    /// Tells the worker in flight that nobody wants its answer any more.
    cancelled: Arc<AtomicBool>,
    next: Option<Instant>,
}

impl Drop for Scan {
    /// A closed window must not leave its SSH sessions open.
    fn drop(&mut self) {
        self.stop();
    }
}

impl Scan {
    /// The saved workspaces last read for `session`; none until a scan has seen it.
    pub(crate) fn workspaces(&self, session: &SessionRef) -> &[SessionWorkspace] {
        self.known.get(session).map_or(&[], Vec::as_slice)
    }

    /// Abandons the scan in flight, as when the Window stops grouping by a
    /// dimension that needs it or closes. The worker is never joined: it
    /// checks the flag around each device call and ends on its own.
    pub(crate) fn stop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if self.pending.take().is_some() {
            self.next = None;
        }
    }

    /// Applies a finished scan and starts the next one when it is due or when
    /// `request` asks for more than the last did. Reports whether the cache
    /// changed.
    pub(crate) fn poll(&mut self, request: &Request, now: Instant) -> bool {
        self.poll_with(request, now, scan)
    }

    pub(crate) fn poll_with(
        &mut self,
        request: &Request,
        now: Instant,
        run: impl FnOnce(Request, Arc<AtomicBool>) -> Vec<DeviceAnswer> + Send + 'static,
    ) -> bool {
        let mut changed = false;
        match self.pending.as_ref().map(mpsc::Receiver::try_recv) {
            Some(Ok(answers)) => {
                self.pending = None;
                self.next = Some(now + INTERVAL);
                self.apply(answers);
                changed = true;
            }
            // A worker that died reports nothing; the old answers stand.
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                tracing::warn!("The sidebar's session scan stopped unexpectedly");
                self.pending = None;
                self.next = Some(now + INTERVAL);
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
        if self.pending.is_none()
            && !request.sessions.is_empty()
            && (self.next.is_none_or(|at| now >= at)
                || !self
                    .asked
                    .as_ref()
                    .is_some_and(|asked| asked.covers(request)))
        {
            self.start(request.clone(), now, run);
        }
        changed
    }

    fn start(
        &mut self,
        request: Request,
        now: Instant,
        run: impl FnOnce(Request, Arc<AtomicBool>) -> Vec<DeviceAnswer> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        self.cancelled = cancelled.clone();
        self.asked = Some(request.clone());
        match std::thread::Builder::new()
            .name("herdr-gui-groups".into())
            .spawn(move || {
                let _ = tx.send(run(request, cancelled));
            }) {
            Ok(_) => self.pending = Some(rx),
            Err(error) => {
                tracing::warn!(%error, "Cannot start the sidebar's session scan");
                self.next = Some(now + INTERVAL);
            }
        }
    }

    fn apply(&mut self, answers: Vec<DeviceAnswer>) {
        for (device, answer) in answers {
            match answer {
                Ok(sessions) => {
                    self.known.retain(|session, _| session.device != device);
                    for (name, workspaces) in sessions {
                        self.known
                            .insert(SessionRef::new(device.clone(), name), workspaces);
                    }
                }
                // A failed device keeps its last answer: stale grouping beats
                // sessions jumping to "No project" while a host is unreachable.
                Err(error) => {
                    let cause = std::error::Error::source(&error);
                    tracing::warn!(device = %device.as_str(), %error, ?cause, "Session scan");
                }
            }
        }
    }

    /// Fills the cache directly, for fixtures.
    #[cfg(test)]
    pub(crate) fn seed(&mut self, session: SessionRef, workspaces: Vec<SessionWorkspace>) {
        self.known.insert(session, workspaces);
    }

    #[cfg(test)]
    pub(crate) fn scanning(&self) -> bool {
        self.pending.is_some()
    }
}

/// The real scan: this machine's sessions when the Window lists any, and every
/// remote device at once, each with its own deadline. `cancelled` is checked
/// before each device is asked and after it answers; a call already in flight
/// cannot be interrupted, but nothing new starts and its answer is dropped.
fn scan(request: Request, cancelled: Arc<AtomicBool>) -> Vec<DeviceAnswer> {
    scan_with(request, &cancelled, list_local_sessions, |host| {
        host.list_sessions()
    })
}

pub(super) fn scan_with(
    request: Request,
    cancelled: &AtomicBool,
    local: impl FnOnce(bool) -> herdr_client::Result<Vec<LocalSession>>,
    remote: impl Fn(&RemoteHost) -> herdr_client::Result<Vec<RemoteSession>> + Sync,
) -> Vec<DeviceAnswer> {
    let stopped = || cancelled.load(Ordering::Relaxed);
    let mut answers = Vec::with_capacity(request.hosts.len() + 1);
    if request.wants_local()
        && !stopped()
        && let Ok(device) = DeviceKey::new(crate::endpoint::LOCAL)
    {
        let listed = local(request.development)
            .map(|sessions| {
                sessions
                    .into_iter()
                    .map(|session| (session.name, session.workspaces))
                    .collect()
            })
            .map_err(DeviceError::Local);
        answers.push((device, listed));
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = request
            .hosts
            .iter()
            .map(|(_, host)| {
                let remote = &remote;
                scope.spawn(move || {
                    if stopped() {
                        return Err(DeviceError::Cancelled);
                    }
                    let listed = remote(host).map_err(DeviceError::Remote);
                    if stopped() {
                        return Err(DeviceError::Cancelled);
                    }
                    listed
                })
            })
            .collect();
        for ((device, _), handle) in request.hosts.iter().zip(handles) {
            let answer = match handle.join() {
                Ok(Ok(sessions)) => Ok(sessions
                    .into_iter()
                    .map(|session: RemoteSession| (session.name, session.workspaces))
                    .collect()),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(DeviceError::Stopped),
            };
            answers.push((device.clone(), answer));
        }
    });
    answers
}
