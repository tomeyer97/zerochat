//! The daemon as the one writer: a CLI request queued while it runs is applied by it.

mod common;

use std::time::Duration;

use common::*;
use zsf::model::{Action, EventKind, SliceStatus};

#[test]
fn queued_request_is_applied_by_the_running_daemon() {
    let env = setup_plan(
        &[
            plan_slice("01").checkpoint(),
            plan_slice("02").after(&["01"]),
        ],
        |_| {},
    );
    // 02 keeps running until the test ends it: the daemon cannot finish (and leave the next
    // request to the CLI) before both requests below reached it.
    env.fake.script("02", vec![runs(&[("02.txt", "x")])]);
    let feature = env.feature.clone();
    feature.queue_request(Action::Start).unwrap();
    let fake = env.fake.clone();
    let daemon = std::thread::spawn(move || zsf::daemon::run(feature, fake));

    let state = |id: &str| {
        env.feature
            .load_state()
            .unwrap()
            .slices
            .get(id)
            .map(|s| (s.status, s.approved))
    };
    // 01 is a checkpoint: its run is done and it waits, not merged, for review.
    wait_for("01 waiting for review", || {
        state("01") == Some((SliceStatus::WaitingForReview, false))
    });
    wait_for("the pid file", || env.feature.daemon_pid().is_some());
    // The daemon holds the writer lock: nobody else can write the state now.
    assert!(env.feature.try_writer_lock().unwrap().is_none());
    assert!(
        env.merged_subjects().is_empty(),
        "nothing merged before the review"
    );
    assert_eq!(
        env.fake.launched_slices(),
        ["01"],
        "02 waits for 01's merge"
    );

    let answer = zsf::daemon::submit(
        &env.feature,
        Action::Approve { slice: "01".into() },
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(answer.ok, "{}", answer.message);
    assert_eq!(answer.message, "01 approved: checking and merging");
    let rejected = zsf::daemon::submit(
        &env.feature,
        Action::Retry {
            slice: "02".into(),
            notes: None,
            fresh: false,
        },
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(!rejected.ok);

    // The daemon checks and merges 01, then 02 runs; then everything is merged and the daemon
    // ends by itself.
    wait_for("02 launched", || env.fake.launched_slices() == ["01", "02"]);
    assert_eq!(state("01"), Some((SliceStatus::Merged, true)));
    env.fake.finish(&env.fake.last_run("02"), SUCCEEDED, None);
    daemon.join().unwrap().unwrap();
    assert_eq!(state("02").unwrap().0, SliceStatus::Merged);
    assert_eq!(
        env.merged_subjects(),
        ["feat 01: Slice 01", "feat 02: Slice 02"]
    );
    assert!(env.feature.daemon_pid().is_none());
    let kinds = env.all_event_kinds();
    assert_eq!(kinds.first(), Some(&EventKind::Initialized));
    for kind in [
        EventKind::DaemonStarted,
        EventKind::Approved,
        EventKind::RequestRejected,
    ] {
        assert!(kinds.contains(&kind), "no {kind:?} in {kinds:?}");
    }
    assert_eq!(kinds.last(), Some(&EventKind::DaemonStopped));
    assert!(env.feature.pending_requests().is_empty());
}

#[test]
fn without_a_daemon_the_cli_applies_its_request_itself() {
    let env = setup_plan(&[plan_slice("01")], |_| {});
    env.fake
        .script("01", vec![fails("change_attempts_exhausted")]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    drop(e);
    let answer = zsf::daemon::submit(
        &env.feature,
        Action::Skip { slice: "01".into() },
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(answer.ok, "{}", answer.message);
    assert_eq!(
        env.feature.load_state().unwrap().slices["01"].status,
        SliceStatus::Skipped
    );
}

#[test]
fn answers_nobody_collects_are_dropped_later() {
    let env = setup_plan(&[plan_slice("01")], |_| {});
    // `zsf start` queues Start and never reads the answer.
    env.feature.queue_request(Action::Start).unwrap();
    env.engine().apply_requests_now().unwrap();
    let done = env.feature.requests_dir().join("done");
    let answers = || std::fs::read_dir(&done).unwrap().count();
    assert_eq!(answers(), 1);
    let old =
        std::time::SystemTime::now() - Duration::from_secs(zsf::store::STALE_ANSWER_SECS + 60);
    for entry in std::fs::read_dir(&done).unwrap().flatten() {
        std::fs::File::options()
            .write(true)
            .open(entry.path())
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let answer = zsf::daemon::submit(
        &env.feature,
        Action::SetMaxParallel { max_parallel: 3 },
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(answer.ok, "{}", answer.message);
    assert_eq!(
        answers(),
        0,
        "the stale answer is gone and the new one was read"
    );
}
