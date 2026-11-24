//! A writer that dies at the worst moments, and runs whose controller died: nothing is launched
//! or resumed twice, and nothing runs unrecorded.

mod common;

use common::*;
use zsf::model::{AttemptKind, EventKind, Job, SliceStatus};
use zsf::zeroshot::RunStatus;

#[test]
fn a_launch_whose_run_id_was_never_recorded_is_found_by_its_key() {
    let env = setup_plan(&[plan_slice("01")], |_| {});
    env.fake.script("01", vec![runs(&[("01.txt", "x")])]);
    {
        let mut e = env.started();
        // One tick starts the launch; the writer "dies" before it hears the run id.
        e.tick().unwrap();
        wait_for("the launch", || env.fake.launches().len() == 1);
    }
    let launch = env.fake.launches()[0].clone();
    assert!(
        launch.key_saved_before,
        "the attempt and its key are saved before the run can exist"
    );
    let saved = env.feature.load_state().unwrap();
    let a = &saved.slices["01"].attempts[0];
    assert_eq!(
        (a.run_id.as_deref(), a.launch_key.as_deref()),
        (None, launch.key.as_deref())
    );
    assert!(matches!(saved.slices["01"].job, Some(Job::Launch { .. })));

    let mut e = env.engine();
    e.recover();
    e.run_until_idle().unwrap();
    assert_eq!(env.fake.launches().len(), 1, "no second run");
    let s = slice(&e, "01");
    assert_eq!((s.status, s.attempts.len()), (SliceStatus::Running, 1));
    assert_eq!(
        s.attempts[0].run_id.as_deref(),
        Some(launch.run_id.as_str())
    );
    assert!(s.worktree.as_ref().is_some_and(|w| w.is_dir()));
    let events = env.events();
    assert!(
        events
            .iter()
            .any(|e| e.detail.contains("found by its launch key")),
        "{events:?}"
    );

    env.fake.finish(&launch.run_id, SUCCEEDED, None);
    assert!(e.run_until_idle().unwrap().done);
    let body = git(&env.repo, &["log", "-1", "--format=%B", BRANCH]);
    assert!(
        body.contains(&format!("Zeroshot-Run: {}", launch.run_id)),
        "{body}"
    );
}

#[test]
fn a_launch_that_never_happened_is_redone_with_the_same_key() {
    let env = setup_plan(&[plan_slice("01")], |_| {});
    // The writer died after saving the attempt, before the launch: state says launching, no run.
    {
        let mut e = env.started();
        e.tick().unwrap();
        wait_for("the launch", || env.fake.launches().len() == 1);
    }
    let first = env.fake.launches()[0].clone();
    // Forget the run on the Zeroshot side: the launch never reached it.
    env.fake.forget_everything();
    let mut e = env.engine();
    e.recover();
    assert!(e.run_until_idle().unwrap().done);
    let launches = env.fake.launches();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].key, first.key, "the same attempt, the same key");
    assert_eq!(slice(&e, "01").attempts.len(), 1);
}

#[test]
fn a_resume_whose_successor_was_never_recorded_is_not_repeated() {
    let env = setup_plan(&[plan_slice("01")], |_| {});
    env.fake.script("01", vec![runs(&[("01.txt", "x")])]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    let run = env.fake.last_run("01");
    env.fake.set(
        &run,
        RunStatus::Gone {
            why: "its controller (pid 7) is not running".into(),
        },
    );
    e.tick().unwrap();
    wait_for("the resume", || env.fake.resume_count() == 1);
    drop(e);
    assert_eq!(
        env.feature.load_state().unwrap().slices["01"].job,
        Some(Job::Resume)
    );

    let mut e = env.engine();
    e.recover();
    e.run_until_idle().unwrap();
    let successor = env.fake.successor_of(&run);
    assert_eq!(env.fake.resume_count(), 1, "resumed once");
    let s = slice(&e, "01");
    assert_eq!((s.status, s.attempts.len()), (SliceStatus::Running, 1));
    assert_eq!(s.attempts[0].run_id.as_deref(), Some(successor.as_str()));
    assert_eq!(s.attempts[0].earlier_runs, [run]);

    // Lost while a stopped writer was not looking, and resumed by hand: adopted, not resumed again.
    let by_hand = "run-by-hand".to_owned();
    env.fake.finish(&successor, FAILED, Some("runtime_lost"));
    env.fake.set(&by_hand, RunStatus::Running);
    env.fake.adopt(&successor, &by_hand);
    e.run_until_idle().unwrap();
    assert_eq!(env.fake.resume_count(), 1);
    assert_eq!(
        slice(&e, "01").attempts[0].run_id.as_deref(),
        Some(by_hand.as_str())
    );
    env.fake.finish(&by_hand, SUCCEEDED, None);
    assert!(e.run_until_idle().unwrap().done);
}

#[test]
fn a_run_zeroshot_recorded_as_runtime_lost_is_resumed() {
    let env = setup_plan(&[plan_slice("01")], |_| {});
    env.fake.script(
        "01",
        vec![fails_after_writing(&[("01.txt", "x")], "runtime_lost")],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    let run = env.fake.launches()[0].run_id.clone();
    assert_eq!(env.fake.resumes(), std::slice::from_ref(&run));
    let s = slice(&e, "01");
    assert_eq!(
        s.attempts.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [AttemptKind::First]
    );
    assert_eq!(s.status, SliceStatus::Running);
    assert_eq!(
        env.event_kinds("01"),
        [
            EventKind::Launched,
            EventKind::Interrupted,
            EventKind::Resumed
        ]
    );
}
