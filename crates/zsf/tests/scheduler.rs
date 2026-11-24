//! The scheduler on temp git repositories with a scripted fake Zeroshot.

mod common;

use common::*;
use zsf::model::{
    Action, AttemptKind, AttemptResult, AutoNoteCause, EventKind, SliceEdit, SliceStatus,
};
use zsf::zeroshot::RunStatus;

#[test]
fn dependency_order_and_max_parallel() {
    // 03 needs 01; 04 needs 02 and 03. At most two runs at once.
    let env = setup_plan(
        &[
            plan_slice("01"),
            plan_slice("02"),
            plan_slice("03").after(&["01"]),
            plan_slice("04").after(&["02", "03"]),
        ],
        |c| c.limits.max_parallel = 2,
    );
    for id in ["01", "02", "03", "04"] {
        env.fake
            .script(id, vec![runs(&[(&format!("{id}.txt"), id)])]);
    }
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert_eq!(env.fake.launched_set(), ["01", "02"]);

    env.succeed(&mut e, "02");
    // 02 merged, but 04 still waits for 03, and 03 for 01.
    assert_eq!(status(&e, "02"), SliceStatus::Merged);
    assert_eq!(env.fake.launched_set(), ["01", "02"]);

    env.succeed(&mut e, "01");
    assert_eq!(env.fake.launched_set(), ["01", "02", "03"]);
    env.succeed(&mut e, "03");
    let tick = env.succeed(&mut e, "04");

    assert!(tick.done && !e.state.active);
    assert_eq!(
        env.merged_subjects(),
        [
            "feat 02: Slice 02",
            "feat 01: Slice 01",
            "feat 03: Slice 03",
            "feat 04: Slice 04"
        ]
    );
    let body = git(&env.repo, &["log", "-1", "--format=%B", BRANCH]);
    assert!(
        body.contains("Slice: 04")
            && body.contains(&format!("Zeroshot-Run: {}", env.fake.last_run("04"))),
        "{body}"
    );
    // Worktrees and slice branches are gone after merging.
    assert!(git(&env.repo, &["branch", "--list", "zsf/*"]).is_empty());
    assert!(e.state.slices.values().all(|s| s.worktree.is_none()));
}

#[test]
fn checkpoint_waits_unmerged_for_review_then_approve_checks_and_merges() {
    let env = setup_plan(
        &[
            plan_slice("01").checkpoint(),
            plan_slice("02").after(&["01"]),
        ],
        |c| c.hooks.gate = Some("echo gate ran >> \"$ZSF_STATE_DIR/gate.log\"".into()),
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    // The run is done, but nothing merged: the worktree, branch and attempt wait for review.
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::WaitingForReview);
    assert_eq!(s.display_status(true), "waiting_for_review");
    assert!(s.worktree.as_ref().unwrap().join("01.txt").is_file());
    assert!(s.commit.is_none() && !s.approved);
    assert_eq!(s.attempts[0].result, None);
    assert!(s.attempts[0].ended_ms.is_some());
    assert!(env.merged_subjects().is_empty());
    assert!(
        !env.feature.dir.join("gate.log").exists(),
        "no gate before the review"
    );
    assert_eq!(env.fake.launched_slices(), ["01"], "02 waits for the merge");
    assert_eq!(e.state.note.as_deref(), Some("waiting for you: review 01"));
    assert!(!e.run_until_idle().unwrap().done);
    let waiting = env
        .events()
        .into_iter()
        .find(|ev| ev.kind == EventKind::CheckpointWaiting)
        .unwrap();
    assert_eq!(
        waiting.detail,
        "waiting for review, not merged (approve = check and merge)"
    );

    assert!(
        e.apply(&Action::Approve { slice: "02".into() }).is_err(),
        "02 is no checkpoint"
    );
    assert_eq!(
        e.apply(&Action::Approve { slice: "01".into() }).unwrap(),
        "01 approved: checking and merging"
    );
    let tick = e.run_until_idle().unwrap();
    assert!(tick.done);
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::Merged);
    assert!(s.approved && s.worktree.is_none());
    assert_eq!(s.attempts[0].result, Some(AttemptResult::Merged));
    assert_eq!(
        std::fs::read_to_string(env.feature.dir.join("gate.log")).unwrap(),
        "gate ran\ngate ran\n",
        "the gate ran for 01 (after the approval) and 02"
    );
    assert_eq!(
        env.merged_subjects(),
        ["feat 01: Slice 01", "feat 02: Slice 02"]
    );
    assert_eq!(
        env.event_kinds("01"),
        [
            EventKind::Launched,
            EventKind::RunFinished,
            EventKind::CheckpointWaiting,
            EventKind::Approved,
            EventKind::Merged
        ]
    );
    assert_eq!(
        e.apply(&Action::Approve { slice: "01".into() }).unwrap(),
        "01 was already approved"
    );
}

#[test]
fn request_changes_reruns_in_the_worktree_with_notes_and_waits_again() {
    let env = setup_plan(
        &[
            plan_slice("01").checkpoint(),
            plan_slice("02").after(&["01"]),
        ],
        |_| {},
    );
    env.fake.script(
        "01",
        vec![
            succeeds(&[("first.txt", "one")]),
            succeeds(&[("second.txt", "two")]),
        ],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "01"), SliceStatus::WaitingForReview);
    let worktree = slice(&e, "01").worktree.clone().unwrap();

    let answer = e
        .apply(&Action::Retry {
            slice: "01".into(),
            notes: Some("Name the file greeting.txt.".into()),
            fresh: false,
        })
        .unwrap();
    assert_eq!(answer, "01: attempt 2 started");
    e.run_until_idle().unwrap();
    let s = slice(&e, "01");
    // The new attempt ran in the same worktree, with the notes, and waits for review again.
    assert_eq!(s.status, SliceStatus::WaitingForReview);
    assert_eq!(
        s.attempts.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [AttemptKind::First, AttemptKind::ManualRetry]
    );
    assert_eq!(s.attempts[0].result, Some(AttemptResult::ChangesRequested));
    assert_eq!(
        s.attempts[0].reason.as_deref(),
        Some("changes requested: Name the file greeting.txt.")
    );
    assert_eq!(s.attempts[1].result, None);
    let second = &env.fake.launches()[1];
    assert_eq!(second.worktree, worktree);
    assert!(second.files_at_start.contains(&"first.txt".to_owned()));
    assert!(
        second.task.contains("## Notes from the developer")
            && second.task.contains("Name the file greeting.txt.")
    );
    assert!(env.merged_subjects().is_empty());
    assert_eq!(env.fake.launched_slices(), ["01", "01"]);
    let retried = env
        .events()
        .into_iter()
        .find(|ev| ev.kind == EventKind::Retried)
        .unwrap();
    assert_eq!(
        retried.detail,
        "changes requested, with notes: Name the file greeting.txt."
    );

    e.apply(&Action::Approve { slice: "01".into() }).unwrap();
    assert!(e.run_until_idle().unwrap().done);
    assert_eq!(
        git(&env.repo, &["show", &format!("{BRANCH}:second.txt")]),
        "two"
    );
    assert_eq!(status(&e, "02"), SliceStatus::Merged);
}

#[test]
fn skipping_a_slice_waiting_for_review_releases_its_dependents() {
    let env = setup_plan(
        &[
            plan_slice("01").checkpoint(),
            plan_slice("02").after(&["01"]),
        ],
        |_| {},
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "01"), SliceStatus::WaitingForReview);
    assert!(
        e.apply(&Action::Accept { slice: "01".into() }).is_err(),
        "accept is for stopped slices"
    );
    e.apply(&Action::Skip { slice: "01".into() }).unwrap();
    assert!(e.run_until_idle().unwrap().done);
    assert_eq!(status(&e, "01"), SliceStatus::Skipped);
    assert_eq!(
        slice(&e, "01").attempts[0].result,
        Some(AttemptResult::Skipped)
    );
    assert_eq!(status(&e, "02"), SliceStatus::Merged);
    assert_eq!(env.merged_subjects(), ["feat 02: Slice 02"]);
}

#[test]
fn an_approved_slice_that_conflicts_reruns_and_merges_without_a_second_review() {
    let env = setup_plan(&[plan_slice("01").checkpoint(), plan_slice("02")], |c| {
        c.limits.max_parallel = 2
    });
    env.fake.script(
        "01",
        vec![
            runs(&[("shared.txt", "from 01\n")]),
            succeeds(&[("shared.txt", "from 02\nfrom 01\n")]),
        ],
    );
    env.fake
        .script("02", vec![runs(&[("shared.txt", "from 02\n")])]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    env.succeed(&mut e, "01");
    assert_eq!(status(&e, "01"), SliceStatus::WaitingForReview);
    env.succeed(&mut e, "02");
    assert_eq!(status(&e, "02"), SliceStatus::Merged);

    e.apply(&Action::Approve { slice: "01".into() }).unwrap();
    assert!(e.run_until_idle().unwrap().done);
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::Merged);
    assert!(s.approved);
    assert_eq!(
        s.attempts
            .iter()
            .map(|a| (a.kind, a.result))
            .collect::<Vec<_>>(),
        [
            (AttemptKind::First, Some(AttemptResult::Conflict)),
            (AttemptKind::Rerun, Some(AttemptResult::Merged))
        ]
    );
    let waits = env
        .event_kinds("01")
        .iter()
        .filter(|k| **k == EventKind::CheckpointWaiting)
        .count();
    assert_eq!(waits, 1, "asked once, before the approval");
    assert_eq!(
        git(&env.repo, &["show", &format!("{BRANCH}:shared.txt")]),
        "from 02\nfrom 01"
    );
}

#[test]
fn a_state_file_with_a_merged_unapproved_checkpoint_still_loads_and_approves() {
    let env = setup_plan(
        &[
            plan_slice("01").checkpoint(),
            plan_slice("02").after(&["01"]),
        ],
        |_| {},
    );
    // What an older zsf wrote: the checkpoint merged before its review, not approved yet.
    std::fs::write(
        env.feature.dir.join("state.json"),
        r#"{"version":1,"active":true,"consecutive_infra":0,"slices":{"01":{"status":"merged","commit":"0123456789abcdef0123456789abcdef01234567","attempts":[{"n":1,"kind":"first","task_file":"t.md","started_ms":1,"ended_ms":2,"result":"merged"}],"infra_failures":0,"conflict_reruns":0}},"updated_ms":2}"#,
    )
    .unwrap();
    let mut e = env.engine();
    e.run_until_idle().unwrap();
    let s = slice(&e, "01");
    assert_eq!(s.display_status(true), "waiting_for_approval");
    assert_eq!(s.gate_reruns, 0);
    assert!(
        env.fake.launched_slices().is_empty(),
        "02 waits for the approval"
    );
    assert_eq!(e.state.note.as_deref(), Some("waiting for you: review 01"));

    assert_eq!(
        e.apply(&Action::Approve { slice: "01".into() }).unwrap(),
        "01 approved"
    );
    assert!(e.run_until_idle().unwrap().done);
    assert_eq!(env.fake.launched_slices(), ["02"]);
    assert_eq!(slice(&e, "01").display_status(true), "merged");
}

#[test]
fn quality_failure_then_retry_with_notes() {
    let env = setup_plan(&[plan_slice("01"), plan_slice("02").after(&["01"])], |_| {});
    env.fake.script(
        "01",
        vec![fails_after_writing(
            &[("half.txt", "wip")],
            "change_attempts_exhausted",
        )],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "01"), SliceStatus::Failed);
    assert!(
        slice(&e, "01")
            .reason
            .as_deref()
            .unwrap()
            .contains("review loop exhausted")
    );
    assert_eq!(
        slice(&e, "01").attempts[0].result,
        Some(AttemptResult::QualityFailed)
    );
    assert_eq!(e.state.note.as_deref(), Some("waiting for you: fix 01"));
    let worktree = slice(&e, "01").worktree.clone().unwrap();

    e.apply(&Action::Retry {
        slice: "01".into(),
        notes: Some("Reuse the parser in src/parse.rs.".into()),
        fresh: false,
    })
    .unwrap();
    e.run_until_idle().unwrap();
    let launches = env.fake.launches();
    assert_eq!(launches.len(), 3);
    let (first, second) = (&launches[0], &launches[1]);
    assert!(!first.task.contains("Reuse the parser"));
    assert!(
        second.task.contains("## Notes from the developer")
            && second.task.contains("Reuse the parser in src/parse.rs.")
    );
    // Same worktree: the earlier attempt's work is still there.
    assert_eq!(second.worktree, worktree);
    assert!(second.files_at_start.contains(&"half.txt".to_owned()));
    // The task files of both attempts are kept for diffing.
    let s = slice(&e, "01");
    assert_eq!(
        s.attempts.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [AttemptKind::First, AttemptKind::ManualRetry]
    );
    assert!(
        std::fs::read_to_string(&s.attempts[1].task_file)
            .unwrap()
            .contains("Reuse the parser")
    );
    assert!(
        !std::fs::read_to_string(&s.attempts[0].task_file)
            .unwrap()
            .contains("Reuse the parser")
    );
    assert_eq!(status(&e, "01"), SliceStatus::Merged);
    assert_eq!(status(&e, "02"), SliceStatus::Merged);
}

#[test]
fn retry_fresh_starts_from_the_branch_tip() {
    let env = setup_plan(&independent(&["01"]), |_| {});
    env.fake.script(
        "01",
        vec![fails_after_writing(
            &[("half.txt", "wip")],
            "change_attempts_exhausted",
        )],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert!(
        e.apply(&Action::Retry {
            slice: "02".into(),
            notes: None,
            fresh: true
        })
        .is_err()
    );
    e.apply(&Action::Retry {
        slice: "01".into(),
        notes: None,
        fresh: true,
    })
    .unwrap();
    e.run_until_idle().unwrap();
    let second = &env.fake.launches()[1];
    assert!(
        !second.files_at_start.contains(&"half.txt".to_owned()),
        "{:?}",
        second.files_at_start
    );
    assert!(slice(&e, "01").attempts[1].fresh);
    assert_eq!(
        slice(&e, "01").attempts[0].result,
        Some(AttemptResult::QualityFailed)
    );
    assert_eq!(status(&e, "01"), SliceStatus::Merged);
}

#[test]
fn accept_merges_a_worktree_finished_by_hand() {
    let env = setup_plan(&independent(&["01"]), |_| {});
    env.fake
        .script("01", vec![fails("change_attempts_exhausted")]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert!(e.apply(&Action::Accept { slice: "01".into() }).is_ok());
    // Nothing changed in the worktree yet: accepting fails and the slice stays stopped.
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "01"), SliceStatus::Failed);

    let worktree = slice(&e, "01").worktree.clone().unwrap();
    std::fs::write(worktree.join("by-hand.txt"), "done by a person\n").unwrap();
    e.apply(&Action::Accept { slice: "01".into() }).unwrap();
    e.run_until_idle().unwrap();
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::Merged);
    assert!(s.accepted);
    assert_eq!(
        git(&env.repo, &["show", &format!("{BRANCH}:by-hand.txt")]),
        "done by a person"
    );
    assert!(env.event_kinds("01").contains(&EventKind::Accepted));
    assert!(!worktree.exists());
}

#[test]
fn skip_unblocks_dependents_and_stops_a_running_run() {
    let env = setup_plan(
        &[
            plan_slice("01"),
            plan_slice("02").after(&["01"]),
            plan_slice("03"),
        ],
        |c| c.limits.max_parallel = 1,
    );
    env.fake
        .script("01", vec![fails("change_attempts_exhausted")]);
    env.fake.script("03", vec![runs(&[])]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    // 01 failed, 03 runs; 02 waits for 01.
    assert_eq!(status(&e, "03"), SliceStatus::Running);
    e.apply(&Action::Skip { slice: "03".into() }).unwrap();
    let run3 = env.fake.last_run("03");
    assert_eq!(env.fake.force_stopped(), [run3]);
    e.apply(&Action::Skip { slice: "01".into() }).unwrap();
    let tick = e.run_until_idle().unwrap();
    assert!(tick.done);
    assert_eq!(status(&e, "02"), SliceStatus::Merged);
    assert_eq!(status(&e, "01"), SliceStatus::Skipped);
    // The stopped run was skipped, not replaced by another attempt.
    assert_eq!(
        slice(&e, "03").attempts[0].result,
        Some(AttemptResult::Skipped)
    );
    assert!(
        e.apply(&Action::Skip { slice: "02".into() }).is_err(),
        "merged slices cannot be skipped"
    );
    let done = env
        .events()
        .into_iter()
        .rev()
        .find(|e| e.kind == EventKind::Done)
        .unwrap();
    assert_eq!(done.detail, format!("1 merged into {BRANCH}, 2 skipped"));
}

#[test]
fn conflict_keeps_a_backup_and_reruns_with_the_previous_work() {
    let env = setup_plan(&independent(&["01", "02"]), |c| c.limits.max_parallel = 2);
    env.fake
        .script("01", vec![runs(&[("shared.txt", "from 01\n")])]);
    env.fake.script(
        "02",
        vec![
            runs(&[("shared.txt", "from 02\n")]),
            succeeds(&[("shared.txt", "from 01\nfrom 02\n")]),
        ],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    env.succeed(&mut e, "01");
    env.succeed(&mut e, "02");

    let backup = format!("zsf-backup/{BRANCH}/02-a1");
    assert_eq!(
        git(&env.repo, &["show", &format!("{backup}:shared.txt")]),
        "from 02"
    );
    let s = slice(&e, "02");
    assert_eq!(
        s.attempts.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [AttemptKind::First, AttemptKind::Rerun]
    );
    assert_eq!(s.attempts[0].result, Some(AttemptResult::Conflict));
    // Who moved the branch, where, and where the work was kept.
    assert_eq!(
        s.attempts[0].reason.as_deref(),
        Some("conflicts with 01 in shared.txt")
    );
    assert_eq!(s.attempts[0].backup.as_deref(), Some(backup.as_str()));
    // The previous work does not apply cleanly, so the worker is told to re-apply it, under
    // zsf's own heading (no person wrote it).
    let rerun = env.fake.launches().last().unwrap().clone();
    assert!(
        rerun.task.contains("git cherry-pick") && rerun.task.contains(&backup),
        "{}",
        rerun.task
    );
    assert!(
        rerun.task.contains("## Earlier attempt (noted by zsf)")
            && !rerun.task.contains("Notes from the developer"),
        "{}",
        rerun.task
    );
    assert!(s.notes.is_empty() && s.auto_notes.len() == 1);
    assert_eq!(s.auto_notes[0].cause, AutoNoteCause::Conflict);
    assert_eq!(s.status, SliceStatus::Merged);
    assert_eq!(
        git(&env.repo, &["show", &format!("{BRANCH}:shared.txt")]),
        "from 01\nfrom 02"
    );
    assert!(env.event_kinds("02").contains(&EventKind::Conflict));
}

#[test]
fn a_second_conflict_stops_the_slice() {
    let env = setup_plan(&independent(&["01", "02", "03"]), |c| {
        c.limits.max_parallel = 3
    });
    env.fake.script("01", vec![runs(&[("shared.txt", "01\n")])]);
    env.fake.script(
        "02",
        vec![
            runs(&[("shared.txt", "02\n")]),
            runs(&[("shared.txt", "02 again\n")]),
        ],
    );
    env.fake.script("03", vec![runs(&[("shared.txt", "03\n")])]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    env.succeed(&mut e, "01");
    env.succeed(&mut e, "02");
    // 02 conflicted with 01 and runs again; the branch then moves under the re-run as well.
    assert_eq!(status(&e, "02"), SliceStatus::Running);
    env.fail(&mut e, "03", "change_attempts_exhausted");
    // Move the branch under 02's rerun by hand.
    let other = env.tmp.path().join("other");
    git(
        &env.repo,
        &["worktree", "add", "-q", &other.to_string_lossy(), BRANCH],
    );
    std::fs::write(other.join("shared.txt"), "by hand\n").unwrap();
    git(&other, &["commit", "-qam", "hand edit"]);
    git(
        &env.repo,
        &["worktree", "remove", "--force", &other.to_string_lossy()],
    );
    env.succeed(&mut e, "02");
    let s = slice(&e, "02");
    assert_eq!(s.status, SliceStatus::Failed);
    assert!(
        s.reason.as_deref().unwrap().starts_with("conflict"),
        "{:?}",
        s.reason
    );
    assert!(
        s.worktree.as_ref().unwrap().is_dir(),
        "the worktree is kept"
    );
}

/// The gate passes only when the worktree has `GATE_OK`.
const GATE_NEEDS_OK: &str =
    "test -f GATE_OK || { echo \"no GATE_OK in $ZSF_WORKTREE for $ZSF_SLICE\"; exit 1; }";

#[test]
fn gate_failure_reruns_once_with_the_gate_output_then_stops_before_merging() {
    let env = setup_plan(&independent(&["01"]), |c| {
        c.hooks.gate = Some(GATE_NEEDS_OK.into());
        c.artifacts = vec!["report.md".into()];
    });
    env.fake.script(
        "01",
        vec![
            succeeds(&[("report.md", "# report\n")]),
            succeeds(&[("report.md", "# report, again\n")]),
        ],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    let s = slice(&e, "01");
    // The first failure re-ran the slice once, in its worktree, with the gate's output as notes.
    let launches = env.fake.launches();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1].worktree, launches[0].worktree);
    assert!(
        launches[1]
            .task
            .contains("## Earlier attempt (noted by zsf)")
            && launches[1]
                .task
                .contains("The merge gate failed on attempt 1")
            && launches[1].task.contains("no GATE_OK in")
            && !launches[1].task.contains("Notes from the developer"),
        "{}",
        launches[1].task
    );
    assert_eq!(
        s.attempts
            .iter()
            .map(|a| (a.kind, a.result))
            .collect::<Vec<_>>(),
        [
            (AttemptKind::First, Some(AttemptResult::GateFailed)),
            (AttemptKind::GateRerun, Some(AttemptResult::GateFailed))
        ]
    );
    assert_eq!(s.gate_reruns, 1);
    // zsf's note says it came from the gate, not a conflict.
    assert_eq!(
        s.auto_notes.iter().map(|n| n.cause).collect::<Vec<_>>(),
        [AutoNoteCause::GateFailed]
    );
    // The second failure stops the slice.
    assert_eq!(s.status, SliceStatus::Failed);
    assert!(
        s.reason.as_deref().unwrap().contains("no GATE_OK in")
            && s.reason.as_deref().unwrap().contains("for 01"),
        "{:?}",
        s.reason
    );
    assert!(env.merged_subjects().is_empty());
    assert!(
        env.feature.artifacts_dir().join("01/report.md").is_file(),
        "declared artifacts are saved"
    );
    let gate_events: Vec<String> = env
        .events()
        .into_iter()
        .filter(|ev| ev.kind == EventKind::GateFailed)
        .map(|ev| ev.detail)
        .collect();
    assert_eq!(gate_events.len(), 2);
    assert!(
        gate_events[0].ends_with("; running it once more with the gate output as notes"),
        "{gate_events:?}"
    );

    // A person fixes it and retries: the gate passes now.
    std::fs::write(s.worktree.as_ref().unwrap().join("GATE_OK"), "").unwrap();
    e.apply(&Action::Retry {
        slice: "01".into(),
        notes: None,
        fresh: false,
    })
    .unwrap();
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "01"), SliceStatus::Merged);
}

#[test]
fn a_gate_rerun_that_fixes_the_gate_merges() {
    let env = setup_plan(&independent(&["01"]), |c| {
        c.hooks.gate = Some(GATE_NEEDS_OK.into());
    });
    env.fake.script(
        "01",
        vec![succeeds(&[("01.txt", "one")]), succeeds(&[("GATE_OK", "")])],
    );
    let mut e = env.started();
    assert!(e.run_until_idle().unwrap().done);
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::Merged);
    assert_eq!(
        s.attempts
            .iter()
            .map(|a| (a.kind, a.result))
            .collect::<Vec<_>>(),
        [
            (AttemptKind::First, Some(AttemptResult::GateFailed)),
            (AttemptKind::GateRerun, Some(AttemptResult::Merged))
        ]
    );
}

#[test]
fn without_gate_reruns_a_gate_failure_stops_the_slice() {
    let env = setup_plan(&independent(&["01"]), |c| {
        c.hooks.gate = Some(GATE_NEEDS_OK.into());
        c.limits.gate_reruns = 0;
    });
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "01"), SliceStatus::Failed);
    assert_eq!(env.fake.launches().len(), 1);
}

#[test]
fn an_approved_checkpoint_whose_gate_fails_reruns_and_waits_for_review_again() {
    let env = setup_plan(
        &[
            plan_slice("01").checkpoint(),
            plan_slice("02").after(&["01"]),
        ],
        |c| c.hooks.gate = Some(GATE_NEEDS_OK.into()),
    );
    env.fake.script(
        "01",
        vec![succeeds(&[("01.txt", "one")]), succeeds(&[("GATE_OK", "")])],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    e.apply(&Action::Approve { slice: "01".into() }).unwrap();
    e.run_until_idle().unwrap();
    // The gate failed after the approval; the re-run changed the work, so a person looks again.
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::WaitingForReview);
    assert!(!s.approved);
    assert_eq!(
        s.attempts.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [AttemptKind::First, AttemptKind::GateRerun]
    );
    assert_eq!(env.fake.launched_slices(), ["01", "01"]);
    e.apply(&Action::Approve { slice: "01".into() }).unwrap();
    assert!(e.run_until_idle().unwrap().done);
    assert_eq!(status(&e, "01"), SliceStatus::Merged);
    assert_eq!(status(&e, "02"), SliceStatus::Merged);
}

#[test]
fn infra_failures_retry_then_pause_until_unpaused() {
    let env = setup_plan(&independent(&["01"]), |c| {
        c.limits.infra_retries = 2;
        c.limits.pause_after_infra = 2;
    });
    env.fake.script(
        "01",
        vec![
            fails("runtime_failed"),
            Run::LaunchFails("agent CLI missing"),
            succeeds(&[("01.txt", "ok")]),
        ],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert!(
        e.state
            .paused
            .as_deref()
            .unwrap()
            .contains("2 infrastructure failures in a row")
    );
    assert_eq!(status(&e, "01"), SliceStatus::Interrupted);
    assert_eq!(
        env.fake.launches().len(),
        1,
        "the second launch failed before a run started"
    );
    let results: Vec<_> = slice(&e, "01").attempts.iter().map(|a| a.result).collect();
    assert_eq!(
        results,
        [
            Some(AttemptResult::InfraFailed),
            Some(AttemptResult::LaunchFailed)
        ]
    );
    // Paused: nothing starts.
    e.run_until_idle().unwrap();
    assert_eq!(slice(&e, "01").attempts.len(), 2);

    e.apply(&Action::Unpause).unwrap();
    e.run_until_idle().unwrap();
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::Merged);
    assert_eq!(
        s.attempts.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [
            AttemptKind::First,
            AttemptKind::InfraRetry,
            AttemptKind::InfraRetry
        ]
    );
    assert_eq!(e.state.consecutive_infra, 0);
}

#[test]
fn infra_retries_run_out() {
    let env = setup_plan(&independent(&["01"]), |c| {
        c.limits.infra_retries = 1;
        c.limits.pause_after_infra = 0;
    });
    env.fake
        .script("01", vec![fails("runtime_failed"), fails("runtime_failed")]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::Failed);
    assert_eq!(s.attempts.len(), 2);
    assert!(s.reason.as_deref().unwrap().starts_with("infrastructure"));
    assert!(e.state.paused.is_none());
}

#[test]
fn restart_with_a_dead_controller_resumes_once() {
    let env = setup_plan(&independent(&["01", "02"]), |c| c.limits.max_parallel = 2);
    env.fake.script("01", vec![runs(&[])]);
    env.fake
        .script("02", vec![runs(&[]), succeeds(&[("02.txt", "again")])]);
    {
        let mut e = env.started();
        e.run_until_idle().unwrap();
        assert_eq!(status(&e, "01"), SliceStatus::Running);
    }
    // A reboot: both controllers are gone. 01 resumes; 02's resume fails and it relaunches.
    let (run1, run2) = (env.fake.last_run("01"), env.fake.last_run("02"));
    env.fake.set(
        &run1,
        RunStatus::Gone {
            why: "its controller ran before the last reboot".into(),
        },
    );
    env.fake.set(
        &run2,
        RunStatus::Gone {
            why: "its controller (pid 7) is not running".into(),
        },
    );
    env.fake
        .fail_resume(&run2, "run does not have a recoverable workspace");

    let mut e = env.engine();
    e.recover();
    e.run_until_idle().unwrap();
    let mut resumes = env.fake.resumes();
    resumes.sort();
    let mut expected = [run1.clone(), run2.clone()];
    expected.sort();
    assert_eq!(resumes, expected);
    let a = &slice(&e, "01").attempts[0];
    assert_eq!(a.earlier_runs, std::slice::from_ref(&run1));
    let successor = a.run_id.clone().unwrap();
    assert_ne!(successor, run1);
    assert!(a.resume_tried);
    assert_eq!(
        slice(&e, "01").attempts.len(),
        1,
        "a resume is no new attempt"
    );
    assert_eq!(
        env.event_kinds("01"),
        [
            EventKind::Launched,
            EventKind::Interrupted,
            EventKind::Resumed
        ]
    );
    // 02: resume failed → an infrastructure failure → relaunched in its worktree.
    assert_eq!(
        slice(&e, "02")
            .attempts
            .iter()
            .map(|a| a.kind)
            .collect::<Vec<_>>(),
        [AttemptKind::First, AttemptKind::InfraRetry]
    );
    assert!(env.event_kinds("02").contains(&EventKind::ResumeFailed));
    assert_eq!(status(&e, "02"), SliceStatus::Merged);

    // Lost a second time: no second resume, it counts as an infrastructure failure.
    env.fake.set(
        &successor,
        RunStatus::Gone {
            why: "gone again".into(),
        },
    );
    e.run_until_idle().unwrap();
    assert_eq!(env.fake.resume_count(), 2);
    assert_eq!(slice(&e, "01").attempts.len(), 2);
    assert_eq!(status(&e, "01"), SliceStatus::Merged);
}

#[test]
fn edit_changes_only_pending_slices() {
    let env = setup_plan(&independent(&["01", "02", "03"]), |c| {
        c.limits.max_parallel = 1
    });
    env.fake.script("03", vec![runs(&[("03.txt", "x")])]);
    let mut e = env.started();
    let spec = env.tmp.path().join("new-02.md");
    std::fs::write(&spec, "Do the new 02.\n").unwrap();
    let edit = |e: &mut zsf::engine::Engine, id: &str, edit: SliceEdit| {
        e.apply(&Action::Edit {
            slice: id.into(),
            edit,
        })
    };
    edit(
        &mut e,
        "03",
        SliceEdit {
            before: Some("01".into()),
            ..Default::default()
        },
    )
    .unwrap();
    edit(
        &mut e,
        "02",
        SliceEdit {
            title: Some("Renamed".into()),
            spec: Some(spec.clone()),
            deps: Some(vec!["03".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        edit(
            &mut e,
            "01",
            SliceEdit {
                deps: Some(vec!["02".into(), "09".into()]),
                ..Default::default()
            }
        )
        .is_err(),
        "unknown dep"
    );
    assert!(
        edit(
            &mut e,
            "03",
            SliceEdit {
                deps: Some(vec!["02".into()]),
                ..Default::default()
            }
        )
        .is_err(),
        "cycle"
    );

    let plan = env.plan();
    assert_eq!(
        plan.slices
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["03", "01", "02"]
    );
    assert_eq!(plan.get("02").unwrap().title, "Renamed");
    e.run_until_idle().unwrap();
    assert_eq!(
        env.fake.launched_slices(),
        ["03"],
        "the slice moved first runs first"
    );
    assert!(
        edit(
            &mut e,
            "03",
            SliceEdit {
                title: Some("x".into()),
                ..Default::default()
            }
        )
        .is_err(),
        "03 has started"
    );

    env.succeed(&mut e, "03");
    assert_eq!(env.fake.launched_slices(), ["03", "01", "02"]);
    assert!(env.fake.launches()[2].task.contains("Do the new 02."));
    assert_eq!(env.merged_subjects().last().unwrap(), "feat 02: Renamed");
}

#[test]
fn a_lease_holds_slices_back() {
    // 01 and 02 share the one "box"; 03 needs none.
    let env = setup_plan(
        &[
            plan_slice("01").leases(&["box"]),
            plan_slice("02").leases(&["box"]),
            plan_slice("03"),
        ],
        |c| {
            c.limits.max_parallel = 3;
            c.leases.insert("box".into(), 1);
        },
    );
    env.fake.script("01", vec![runs(&[])]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert_eq!(env.fake.launched_set(), ["01", "03"]);
    assert_eq!(e.state.note.as_deref(), Some("02 waits for lease box"));
    env.succeed(&mut e, "01");
    assert_eq!(env.fake.launched_slices()[2], "02");
}

#[test]
fn low_disk_space_holds_slices_back() {
    let env = setup_plan(&independent(&["01"]), |c| c.limits.min_free_gb = 25.0);
    let mut e = env.started();
    e.set_free_space(|_| 3.0);
    e.run_until_idle().unwrap();
    assert!(env.fake.launches().is_empty());
    assert_eq!(
        e.state.note.as_deref(),
        Some("disk below 25 GB (3 GB free) before 01")
    );
}

#[test]
fn a_conflict_rerun_takes_the_slot_its_slice_held() {
    // 01 and 02 edit the same file; 03 and 04 run long. At most two runs at once, ever.
    let env = setup_plan(&independent(&["01", "02", "03", "04"]), |c| {
        c.limits.max_parallel = 2
    });
    env.fake.script(
        "01",
        vec![
            runs(&[("shared.txt", "from 01\n")]),
            runs(&[("shared.txt", "from 01 again\n")]),
        ],
    );
    env.fake
        .script("02", vec![runs(&[("shared.txt", "from 02\n")])]);
    env.fake.script("03", vec![runs(&[])]);
    env.fake.script("04", vec![runs(&[])]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    env.succeed(&mut e, "02");
    assert_eq!(env.fake.launched_set(), ["01", "02", "03"]);
    // 01 finishes, conflicts with 02 and runs again: 04 must not have taken its slot meanwhile.
    env.succeed(&mut e, "01");
    assert_eq!(
        slice(&e, "01")
            .attempts
            .iter()
            .map(|a| a.kind)
            .collect::<Vec<_>>(),
        [AttemptKind::First, AttemptKind::Rerun]
    );
    assert_eq!(env.fake.launched_set(), ["01", "01", "02", "03"]);
    let busy = e
        .state
        .slices
        .values()
        .filter(|s| s.status.is_active())
        .count();
    assert_eq!(busy, 2);
    assert_eq!(env.fake.max_running(), 2, "never more than two runs");
}

#[test]
fn a_retry_waits_while_paused() {
    // The retry is queued and launches as a manual retry after unpause.
    let env = setup_plan(&independent(&["01"]), |c| {
        c.limits.infra_retries = 0;
        c.limits.pause_after_infra = 1;
    });
    env.fake.script(
        "01",
        vec![
            Run::LaunchFails("connection refused"),
            succeeds(&[("01.txt", "ok")]),
        ],
    );
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert!(e.state.paused.is_some());
    assert_eq!(status(&e, "01"), SliceStatus::Failed);
    assert_eq!(
        slice(&e, "01").reason.as_deref(),
        Some("infrastructure: launch failed: connection refused")
    );
    let message = e
        .apply(&Action::Retry {
            slice: "01".into(),
            notes: Some("try again".into()),
            fresh: true,
        })
        .unwrap();
    assert_eq!(message, "01 retries when the feature is unpaused");
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "01"), SliceStatus::Pending);
    assert_eq!(
        slice(&e, "01").attempts.len(),
        1,
        "nothing launched while paused"
    );
    e.apply(&Action::Unpause).unwrap();
    e.run_until_idle().unwrap();
    let s = slice(&e, "01");
    assert_eq!(s.status, SliceStatus::Merged);
    assert_eq!(
        (s.attempts[1].kind, s.attempts[1].fresh),
        (AttemptKind::ManualRetry, true)
    );
    assert!(s.retry.is_none());
}

#[test]
fn a_retry_waits_while_stopped() {
    // It launches when the feature starts again.
    let env = setup_plan(&independent(&["01"]), |_| {});
    env.fake
        .script("01", vec![fails("change_attempts_exhausted")]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    e.apply(&Action::Stop).unwrap();
    assert_eq!(
        e.apply(&Action::Retry {
            slice: "01".into(),
            notes: None,
            fresh: false
        })
        .unwrap(),
        "01 retries when the feature is started"
    );
    e.run_until_idle().unwrap();
    assert_eq!(env.fake.launches().len(), 1);
    e.apply(&Action::Start).unwrap();
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "01"), SliceStatus::Merged);
    assert_eq!(slice(&e, "01").attempts[1].kind, AttemptKind::ManualRetry);
}

#[test]
fn a_retry_waits_for_a_free_slot() {
    let env = setup_plan(&independent(&["01", "02"]), |c| c.limits.max_parallel = 1);
    env.fake
        .script("01", vec![fails("change_attempts_exhausted")]);
    env.fake.script("02", vec![runs(&[("02.txt", "x")])]);
    let mut e = env.started();
    e.run_until_idle().unwrap();
    assert_eq!(status(&e, "02"), SliceStatus::Running);
    assert_eq!(
        e.apply(&Action::Retry {
            slice: "01".into(),
            notes: None,
            fresh: false
        })
        .unwrap(),
        "01 retries when a slot is free"
    );
    e.run_until_idle().unwrap();
    assert_eq!(
        env.fake.max_running(),
        1,
        "the retry did not exceed max_parallel"
    );
    env.succeed(&mut e, "02");
    assert_eq!(status(&e, "01"), SliceStatus::Merged);
    assert_eq!(status(&e, "02"), SliceStatus::Merged);
}
