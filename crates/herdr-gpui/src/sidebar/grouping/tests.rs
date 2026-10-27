#![allow(clippy::unwrap_used)]

use super::*;

fn id(n: u8) -> ProjectId {
    ProjectId::parse(&format!("00000000-0000-4000-8000-0000000000{n:02}")).unwrap()
}

fn tag(n: u8, name: &str) -> Option<ProjectTag> {
    Some(ProjectTag {
        id: id(n),
        name: name.to_owned(),
    })
}

fn member(item: usize, project: Option<ProjectTag>, status: Status) -> Member {
    Member {
        item,
        project,
        status,
        mr: Bucket::Unknown,
    }
}

fn items(groups: &[Group]) -> Vec<(String, Vec<usize>)> {
    groups
        .iter()
        .map(|group| (group.title.clone(), group.items.clone()))
        .collect()
}

#[test]
fn none_lists_flat() {
    let members = [member(0, tag(1, "app"), Status::Idle)];
    assert!(project(Grouping::None, &members).is_none());
}

#[test]
fn mr_groups_follow_the_bucket_priority_and_skip_empty_ones() {
    let mut members = [
        member(0, None, Status::Idle),
        member(1, None, Status::Idle),
        member(2, None, Status::Idle),
        member(3, None, Status::Idle),
    ];
    for (member, mr) in members.iter_mut().zip([
        Bucket::Merged,
        Bucket::ChecksFailing,
        Bucket::Merged,
        Bucket::Unsupported,
    ]) {
        member.mr = mr;
    }
    let groups = project(Grouping::Mr, &members).unwrap();
    assert_eq!(
        items(&groups),
        [
            ("Checks failing".to_owned(), vec![1]),
            ("Merged".to_owned(), vec![0, 2]),
            ("Unsupported".to_owned(), vec![3]),
        ]
    );
    assert_eq!(groups[0].key, GroupKey::Mr(Bucket::ChecksFailing));
}

#[test]
fn sessions_of_one_project_form_one_group_in_window_order() {
    let members = [
        member(0, tag(1, "app"), Status::Idle),
        member(1, tag(2, "docs"), Status::Idle),
        member(2, tag(1, "app"), Status::Working),
        member(3, tag(1, "app"), Status::Idle),
    ];
    let groups = project(Grouping::Project, &members).unwrap();
    assert_eq!(
        items(&groups),
        [
            ("app".to_owned(), vec![0, 2, 3]),
            ("docs".to_owned(), vec![1])
        ]
    );
}

#[test]
fn projects_sort_by_name_ignoring_case_then_id_and_unclassified_comes_last() {
    let members = [
        member(0, None, Status::Idle),
        member(1, tag(3, "zeta"), Status::Idle),
        member(2, tag(2, "Alpha"), Status::Idle),
        member(3, tag(4, "alpha"), Status::Idle),
        member(4, None, Status::Idle),
    ];
    let groups = project(Grouping::Project, &members).unwrap();
    assert_eq!(
        items(&groups),
        [
            ("Alpha".to_owned(), vec![2]),
            ("alpha".to_owned(), vec![3]),
            ("zeta".to_owned(), vec![1]),
            ("No project".to_owned(), vec![0, 4]),
        ]
    );
    assert_eq!(groups[3].key, GroupKey::Unclassified);
}

#[test]
fn the_order_of_groups_does_not_depend_on_the_order_of_members() {
    let forward = [
        member(0, tag(1, "b"), Status::Idle),
        member(1, tag(2, "a"), Status::Idle),
    ];
    let backward = [forward[1].clone(), forward[0].clone()];
    let titles = |members: &[Member]| -> Vec<String> {
        project(Grouping::Project, members)
            .unwrap()
            .into_iter()
            .map(|group| group.title)
            .collect()
    };
    assert_eq!(titles(&forward), titles(&backward));
}

#[test]
fn status_groups_follow_a_fixed_priority_and_skip_empty_ones() {
    let members = [
        member(0, None, Status::Stopped),
        member(1, None, Status::Idle),
        member(2, None, Status::Working),
        member(3, None, Status::Disconnected),
        member(4, None, Status::Waiting),
        member(5, None, Status::Idle),
    ];
    let groups = project(Grouping::Status, &members).unwrap();
    assert_eq!(
        items(&groups),
        [
            ("Waiting".to_owned(), vec![4]),
            ("Working".to_owned(), vec![2]),
            ("Idle".to_owned(), vec![1, 5]),
            ("Disconnected".to_owned(), vec![3]),
            ("Stopped".to_owned(), vec![0]),
        ]
    );
    let only_idle = project(Grouping::Status, &members[1..2]).unwrap();
    assert_eq!(items(&only_idle), [("Idle".to_owned(), vec![1])]);
}

#[test]
fn group_keys_are_distinct_element_ids() {
    let keys = [
        GroupKey::Unclassified,
        GroupKey::Project(id(1)),
        GroupKey::Status(Status::Idle),
        GroupKey::Status(Status::Working),
        GroupKey::Mr(Bucket::Open),
        GroupKey::Mr(Bucket::NoPr),
    ];
    let slugs: std::collections::HashSet<_> = keys.iter().map(GroupKey::slug).collect();
    assert_eq!(slugs.len(), keys.len());
}

mod scan {
    use super::super::scan::{DeviceAnswer, DeviceError, Request, Scan};
    use herdr_client::{DeviceKey, SessionRef, SessionWorkspace};
    use std::time::{Duration, Instant};

    fn session(device: &str, name: &str) -> SessionRef {
        SessionRef::new(DeviceKey::new(device).unwrap(), name)
    }

    fn workspace(path: &str) -> SessionWorkspace {
        SessionWorkspace {
            project: path.into(),
            path: path.into(),
            branch: None,
            label: None,
            repo: None,
            ..Default::default()
        }
    }

    fn wait(scan: &mut Scan, request: &Request, now: Instant) -> bool {
        for _ in 0..200 {
            if scan.poll_with(request, now, |_, _| unreachable_run()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    fn unreachable_run() -> Vec<DeviceAnswer> {
        Vec::new()
    }

    #[test]
    fn a_scan_fills_the_cache_and_a_covered_request_waits_for_the_interval() {
        let a = session("local", "default");
        let request = Request::new(vec![a.clone()], false, Vec::new());
        let mut scan = Scan::default();
        let now = Instant::now();
        let device = DeviceKey::new("local").unwrap();
        let answer = vec![(
            device,
            Ok(vec![("default".to_owned(), vec![workspace("/a")])]),
        )];
        assert!(!scan.poll_with(&request, now, move |_, _| answer));
        assert!(scan.scanning());
        assert!(wait(&mut scan, &request, now));
        assert_eq!(scan.workspaces(&a), [workspace("/a")]);
        // Inside the interval nothing new starts.
        assert!(!scan.poll_with(&request, now, |_, _| unreachable_run()));
        assert!(!scan.scanning());
    }

    #[test]
    fn a_new_session_in_the_window_scans_again_at_once() {
        let a = session("local", "default");
        let b = session("local", "other");
        let mut scan = Scan::default();
        let now = Instant::now();
        let first = Request::new(vec![a.clone()], false, Vec::new());
        scan.poll_with(&first, now, |_, _| unreachable_run());
        assert!(wait(&mut scan, &first, now));
        assert!(!scan.scanning());
        let second = Request::new(vec![a, b], false, Vec::new());
        scan.poll_with(&second, now, |_, _| unreachable_run());
        assert!(scan.scanning(), "a session the last scan never asked about");
    }

    #[test]
    fn a_failed_device_keeps_its_last_answer() {
        let a = session("local", "default");
        let request = Request::new(vec![a.clone()], false, Vec::new());
        let mut scan = Scan::default();
        scan.seed(a.clone(), vec![workspace("/kept")]);
        let now = Instant::now();
        let device = DeviceKey::new("local").unwrap();
        let failure = vec![(device, Err(DeviceError::Stopped))];
        scan.poll_with(&request, now, move |_, _| failure);
        assert!(wait(&mut scan, &request, now));
        assert_eq!(scan.workspaces(&a), [workspace("/kept")]);
    }

    #[test]
    fn an_empty_window_never_scans() {
        let mut scan = Scan::default();
        scan.poll_with(&Request::default(), Instant::now(), |_, _| {
            unreachable_run()
        });
        assert!(!scan.scanning());
    }

    use super::super::scan::scan_with;
    use herdr_client::{RemoteHost, RemoteSession};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };

    fn io_error() -> herdr_client::Error {
        herdr_client::Error::Io(std::io::Error::other("refused"))
    }

    fn two_hosts() -> Request {
        let hosts = ["a", "b"]
            .into_iter()
            .map(|name| (DeviceKey::new(name).unwrap(), RemoteHost::Ssh(name.into())))
            .collect();
        Request::new(vec![session("a", "x"), session("b", "x")], false, hosts)
    }

    /// A worker that runs until it is told to stop, then reports that it did.
    fn until_cancelled(
        done: mpsc::Sender<()>,
    ) -> impl FnOnce(Request, Arc<AtomicBool>) -> Vec<DeviceAnswer> {
        move |_, cancelled| {
            // Bounded, so a missing cancellation fails the test and not the run.
            for _ in 0..2000 {
                if cancelled.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = done.send(());
            Vec::new()
        }
    }

    fn stopped_within_bound(done: &mpsc::Receiver<()>) -> bool {
        done.recv_timeout(Duration::from_secs(5)).is_ok()
    }

    #[test]
    fn dropping_the_scan_stops_its_worker() {
        let (done, exited) = mpsc::channel();
        let mut scan = Scan::default();
        let request = Request::new(vec![session("local", "default")], false, Vec::new());
        scan.poll_with(&request, Instant::now(), until_cancelled(done));
        assert!(scan.scanning());
        drop(scan);
        assert!(
            stopped_within_bound(&exited),
            "the worker outlived its scan"
        );
    }

    #[test]
    fn stopping_the_scan_abandons_it_and_a_later_request_scans_again() {
        let (done, exited) = mpsc::channel();
        let mut scan = Scan::default();
        let request = Request::new(vec![session("local", "default")], false, Vec::new());
        let now = Instant::now();
        scan.poll_with(&request, now, until_cancelled(done));
        scan.stop();
        assert!(!scan.scanning());
        assert!(stopped_within_bound(&exited));
        // Stopping must not leave the scan waiting out the interval.
        scan.poll_with(&request, now, |_, _| unreachable_run());
        assert!(scan.scanning());
        assert!(wait(&mut scan, &request, now));
    }

    #[test]
    fn a_cancelled_scan_asks_no_further_device() {
        let calls = AtomicUsize::new(0);
        let cancelled = AtomicBool::new(true);
        let answers = scan_with(
            two_hosts(),
            &cancelled,
            |_| Ok(Vec::new()),
            |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(Vec::<RemoteSession>::new())
            },
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(answers.len(), 2);
        assert!(
            answers
                .iter()
                .all(|(_, answer)| matches!(answer, Err(DeviceError::Cancelled)))
        );
    }

    #[test]
    fn an_answer_that_lands_after_cancellation_is_dropped() {
        let cancelled = AtomicBool::new(false);
        let answers = scan_with(
            two_hosts(),
            &cancelled,
            |_| Ok(Vec::new()),
            |_| {
                cancelled.store(true, Ordering::Relaxed);
                Ok(Vec::<RemoteSession>::new())
            },
        );
        assert!(
            answers
                .iter()
                .all(|(_, answer)| matches!(answer, Err(DeviceError::Cancelled)))
        );
    }

    #[test]
    fn device_failures_keep_their_variant_and_source() {
        let request = {
            let host = (DeviceKey::new("a").unwrap(), RemoteHost::Ssh("a".into()));
            Request::new(
                vec![session("local", "default"), session("a", "x")],
                false,
                vec![host],
            )
        };
        let answers = scan_with(
            request,
            &AtomicBool::new(false),
            |_| Err(io_error()),
            |_| Err(io_error()),
        );
        let kinds: Vec<_> = answers
            .iter()
            .map(|(device, answer)| (device.as_str().to_owned(), answer))
            .collect();
        match kinds.as_slice() {
            [
                (local, Err(DeviceError::Local(inner))),
                (remote, Err(DeviceError::Remote(_))),
            ] => {
                assert_eq!((local.as_str(), remote.as_str()), ("local", "a"));
                assert!(matches!(inner, herdr_client::Error::Io(_)));
            }
            _ => panic!("unexpected answers"),
        }
        let error = answers
            .into_iter()
            .find_map(|(_, answer)| answer.err())
            .unwrap();
        let source = std::error::Error::source(&error).unwrap();
        assert!(source.to_string().contains("refused"));
    }
}
