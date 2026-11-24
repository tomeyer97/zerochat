//! The batches list and one batch's page: real batches and label pseudo-batches, their identity
//! and totals, and a batch whose runner died.

use std::path::Path;

use super::super::batch::reconciled_status;
use super::super::*;
use super::{Batch, Run, seed_db};

/// The status and verdict enums write exactly the strings the API always had; a status pb
/// records that zerochat does not know passes through as pb wrote it.
#[test]
fn status_and_noise_keep_their_wire_strings() {
    for status in [
        "running",
        "done",
        "stopped",
        "crashed",
        "starting",
        "failed_to_start",
        "paused",
        "",
    ] {
        assert_eq!(
            serde_json::to_value(BatchStatus::from_pb(status)).unwrap(),
            status
        );
    }
    assert_eq!(
        BatchStatus::from_pb("paused"),
        BatchStatus::Other("paused".to_owned())
    );
    let noise = [
        Noise::WithinNoise,
        Noise::BeyondSpread,
        Noise::NeedsSeeds,
        Noise::NotApplicable,
    ];
    assert_eq!(
        serde_json::to_value(noise).unwrap(),
        serde_json::json!(["within_noise", "beyond_spread", "needs_seeds", "n_a"])
    );
}

#[test]
fn pseudo_batches_group_by_label_without_a_batches_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, false);
    Run::new("r1", "baseline", "gron", 1, "success")
        .eval("ok", Some(0.85))
        .at(1000.0)
        .insert(&conn);
    Run::new("r2", "baseline", "gron", 2, "running")
        .at(1001.0)
        .insert(&conn);
    drop(conn);

    let batches = list_batches(dir.path()).unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].id, "label:baseline");
    assert!(!batches[0].real);
    assert_eq!(batches[0].runs_total, 2);
    assert_eq!(batches[0].runs_done, 1);
    assert_eq!(batches[0].task_count, 1);
    assert_eq!(batches[0].seed_count, 2);
    // harness_version is the harness's own short version hash, not the CLI build; the CLI
    // build is still available, separately, as zeroshot_version.
    assert_eq!(batches[0].harness_version, Some("v1".to_owned()));
    assert_eq!(batches[0].zeroshot_version, Some("11.2.0".to_owned()));

    let detail = batch_detail(dir.path(), "label:baseline").unwrap().unwrap();
    assert_eq!(detail.tasks, vec!["gron".to_owned()]);
    assert_eq!(detail.seeds, vec![1, 2]);
    assert_eq!(detail.cells.len(), 2);
    let scored = detail.cells.iter().find(|c| c.seed == 1).unwrap();
    assert_eq!(scored.state, "scored");
    assert_eq!(scored.score, Some(0.85));
    let running = detail.cells.iter().find(|c| c.seed == 2).unwrap();
    assert_eq!(running.state, "running_agent");
    assert_eq!(detail.status, BatchStatus::Running);
}

#[test]
fn real_batches_are_used_when_the_table_exists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, true);
    Batch::new("b1", "try-planner")
        .tasks(&["gron"])
        .seeds(&[1, 2])
        .insert(&conn);
    Run::new("r1", "try-planner", "gron", 1, "success")
        .eval("ok", Some(0.9))
        .at(1000.0)
        .in_batch("b1")
        .insert(&conn);
    Run::new("r2", "try-planner", "gron", 2, "success")
        .eval("error", None)
        .at(1001.0)
        .in_batch("b1")
        .insert(&conn);
    // A pre-existing run with no batch: falls back to a label pseudo-batch (same database).
    Run::new("r3", "orphan", "other", 1, "success")
        .eval("ok", Some(0.5))
        .at(900.0)
        .insert(&conn);
    drop(conn);

    let batches = list_batches(dir.path()).unwrap();
    assert_eq!(batches.len(), 2);
    let real = batches.iter().find(|b| b.id == "b1").unwrap();
    assert!(real.real);
    assert_eq!(real.runs_total, 2);
    assert_eq!(real.mean_score, Some(0.9));
    let pseudo = batches.iter().find(|b| b.id == "label:orphan").unwrap();
    assert!(!pseudo.real);

    let detail = batch_detail(dir.path(), "b1").unwrap().unwrap();
    assert_eq!(detail.cells.len(), 2);
    let failed = detail.cells.iter().find(|c| c.seed == 2).unwrap();
    assert_eq!(failed.state, "failed");
    assert_eq!(failed.exit_reason, Some("error".to_owned()));
    assert_eq!(detail.mean_score, Some(0.9));
}

/// The batch page's identity names the harness (a short version hash) and its models,
/// separately from the Zeroshot CLI build; its limits, parallelism and total spend (over every
/// attempt, not just the cells shown) are there too.
#[test]
fn batch_detail_carries_harness_identity_limits_and_total_spend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, true);
    let runtime = serde_json::json!({"nodes": {
        "build": {"kind": "agent", "model": "claude-sonnet-5"},
        "triage": {"kind": "agent", "model": "claude-haiku-4-5"},
        "join": {"kind": "seq"},
    }});
    conn.execute(
        "UPDATE harness_versions SET runtime_json = ?1 WHERE version_id = 'v1'",
        [runtime.to_string()],
    )
    .unwrap();
    Batch::new("b1", "clone-v1")
        .tasks(&["gron"])
        .seeds(&[0, 1])
        .limits(serde_json::json!({"max_cost_usd": 6.0, "max_wall_minutes": 45, "max_steps": 24}))
        .parallel(2)
        .insert(&conn);
    Run::new("r1", "clone-v1", "gron", 0, "success")
        .scored(0.8)
        .cost(3.0)
        .wall(900.0)
        .in_batch("b1")
        .insert(&conn);
    // An interrupted attempt that was retried: its cost must still count towards the total.
    Run::new("r2a", "clone-v1", "gron", 1, "interrupted")
        .cost(1.5)
        .wall(60.0)
        .in_batch("b1")
        .insert(&conn);
    Run::new("r2b", "clone-v1", "gron", 1, "success")
        .scored(0.7)
        .cost(3.2)
        .wall(950.0)
        .in_batch("b1")
        .insert(&conn);
    drop(conn);

    let detail = batch_detail(dir.path(), "b1").unwrap().unwrap();
    assert_eq!(detail.harness_version, Some("v1".to_owned()));
    assert_eq!(detail.zeroshot_version, Some("11.2.0".to_owned()));
    assert_eq!(
        detail.models,
        vec!["claude-haiku-4-5".to_owned(), "claude-sonnet-5".to_owned()]
    );
    let limits = detail.limits.unwrap();
    assert_eq!(limits.max_cost_usd, Some(6.0));
    assert_eq!(limits.max_wall_minutes, Some(45.0));
    assert_eq!(limits.max_steps, Some(24));
    assert_eq!(detail.parallel, Some(2));
    assert!(
        (detail.total_cost_usd.unwrap() - (3.0 + 1.5 + 3.2)).abs() < 1e-9,
        "the interrupted attempt's cost must still be counted"
    );
    assert!((detail.total_wall_s.unwrap() - (900.0 + 60.0 + 950.0)).abs() < 1e-9);
}

/// A run a limit, a Stop or a crash ended still gets scored on the code it had
/// (`eval_after_limit`); it counts in the mean, and says so: `cut_short` on the cell, and the
/// mean without those runs next to the batch mean.
#[test]
fn scored_runs_that_were_cut_short_are_marked_and_the_mean_without_them_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, false);
    Run::new("a", "cut", "t", 0, "success")
        .eval("ok", Some(0.9))
        .insert(&conn);
    Run::new("b", "cut", "t", 1, "interrupted")
        .eval("ok", Some(0.3))
        .insert(&conn);
    Run::new("c", "cut", "t", 2, "step_limit")
        .eval("ok", Some(0.6))
        .insert(&conn);
    Run::new("d", "cut", "t", 3, "step_limit")
        .eval("skipped", None)
        .insert(&conn);
    drop(conn);
    let detail = batch_detail(dir.path(), "label:cut").unwrap().unwrap();
    let by_seed = |s: i64| detail.cells.iter().find(|c| c.seed == s).unwrap();
    assert_eq!(by_seed(0).cut_short, None);
    assert_eq!(by_seed(1).cut_short.as_deref(), Some("interrupted"));
    assert_eq!(by_seed(1).state, "scored");
    assert_eq!(
        (
            by_seed(2).cut_short.as_deref(),
            by_seed(2).over_limit.as_deref()
        ),
        (Some("steps"), Some("steps"))
    );
    assert_eq!(
        by_seed(3).state,
        "over_steps",
        "an unscored step-limit stop is a limit, not a plain failure"
    );
    assert!(
        (detail.mean_score.unwrap() - 0.6).abs() < 1e-9,
        "cut-short runs ARE in the batch mean"
    );
    assert!((detail.mean_score_finished.unwrap() - 0.9).abs() < 1e-9);
}

#[test]
fn missing_database_is_an_error_not_a_panic() {
    assert!(list_batches(Path::new("/does/not/exist")).is_err());
}

/// A real batch counts its planned grid, so a batch stopped before pb got to a cell reads 1/2
/// done (not 1/1) and is not "in flight".
#[test]
fn a_real_batch_counts_planned_cells() {
    let dir = tempfile::tempdir().unwrap();
    let conn = seed_db(&dir.path().join("pb.sqlite"), true);
    Batch::new("b1", "x")
        .tasks(&["gron"])
        .seeds(&[0, 1])
        .status("stopped")
        .insert(&conn);
    Run::new("r1", "x", "gron", 0, "success")
        .eval("ok", Some(0.5))
        .in_batch("b1")
        .insert(&conn);
    drop(conn);
    let summary = list_batches(dir.path())
        .unwrap()
        .into_iter()
        .find(|b| b.id == "b1")
        .unwrap();
    assert_eq!(
        (
            summary.runs_done,
            summary.runs_total,
            summary.runs_in_flight,
            summary.retried_attempts
        ),
        (1, 2, 0, 0)
    );
    assert_eq!(summary.retried_cost_usd, None);
}

#[test]
fn a_running_batch_row_with_no_runs_yet_is_starting() {
    let dir = tempfile::tempdir().unwrap();
    let conn = seed_db(&dir.path().join("pb.sqlite"), true);
    Batch::new("b1", "fresh")
        .tasks(&["gron"])
        .seeds(&[0])
        .parallel(1)
        .running(std::process::id())
        .insert(&conn);
    drop(conn);
    let detail = batch_detail(dir.path(), "b1").unwrap().unwrap();
    assert!(detail.starting);
    assert_eq!(detail.status, BatchStatus::Running);
    assert_eq!(detail.cells[0].state, "queued");
}

// ---------- a batch whose runner died is reconciled the way pb does ----------

#[test]
fn reconciled_status_matches_pbs_own_reconcile_batches() {
    // Not running: never touched.
    assert_eq!(reconciled_status("done", None, None), BatchStatus::Done);
    assert_eq!(
        reconciled_status("stopped", Some(1), None),
        BatchStatus::Stopped
    );
    // Running with a live pid on this host: stays running.
    let my_pid = std::process::id() as i64;
    assert_eq!(
        reconciled_status("running", Some(my_pid), None),
        BatchStatus::Running
    );
    // Running with no recorded pid at all: pb's own check (`if pid:`) treats that as dead.
    assert_eq!(
        reconciled_status("running", None, None),
        BatchStatus::Crashed
    );
    // Running with a pid that is (as good as certainly) dead on this host: crashed.
    assert_eq!(
        reconciled_status("running", Some(999_999_999), None),
        BatchStatus::Crashed
    );
    // Running on a different host: can't be checked from here, so — like pb — assumed
    // alive rather than called crashed when it might in fact be running elsewhere.
    assert_eq!(
        reconciled_status("running", Some(999_999_999), Some("some-other-host")),
        BatchStatus::Running
    );
}

#[test]
fn a_batch_left_running_by_a_dead_process_reads_as_crashed_and_unblocks_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, true);
    Batch::new("b1", "dead-run")
        .tasks(&["gron"])
        .seeds(&[0])
        .running(999_999_999)
        .on_this_host()
        .insert(&conn);
    // pb's own reconcile_batches would also rewrite this row, but nothing has run `pb` since
    // the process died: it is still `status='running'` on disk.
    Run::new("r1", "dead-run", "gron", 0, "running")
        .at(1000.0)
        .in_batch("b1")
        .insert(&conn);
    drop(conn);

    let batches = list_batches(dir.path()).unwrap();
    let b = batches.iter().find(|b| b.id == "b1").unwrap();
    assert_eq!(
        b.status,
        BatchStatus::Crashed,
        "a dead pid must not keep reading as running (the Start form blocks on 'running')"
    );

    let detail = batch_detail(dir.path(), "b1").unwrap().unwrap();
    assert_eq!(detail.status, BatchStatus::Crashed);
    let cell = detail.cells.iter().find(|c| c.seed == 0).unwrap();
    assert_eq!(
        cell.state, "crashed",
        "a cell whose row is still status='running' must follow the reconciled batch status, not show running_agent forever"
    );
}

/// `running_batches_alive` is what lets the server's cache key (`DbFingerprint`) notice a pid
/// dying even though nothing on disk changes when a process dies: without it, the cached batch
/// list and detail kept saying "running" until something else touched pb.sqlite (see
/// `server::db_fingerprint`). With several batches running at once, any one of them dying must
/// count.
#[test]
fn running_batches_alive_reflects_each_pid_without_any_db_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, true);
    assert_eq!(
        running_batches_alive(dir.path()),
        None,
        "no running batch at all"
    );

    for id in ["b1", "b2"] {
        Batch::new(id, "x")
            .running(std::process::id())
            .on_this_host()
            .insert(&conn);
    }
    assert_eq!(
        running_batches_alive(dir.path()),
        Some((2, 2)),
        "this test process is itself alive"
    );

    conn.execute("UPDATE batches SET pid=999999999 WHERE id='b2'", [])
        .unwrap();
    assert_eq!(
        running_batches_alive(dir.path()),
        Some((2, 1)),
        "one of two concurrent batches died, with no other write to the db"
    );
}

/// Several batches running at once (pb no longer runs one at a time): each lists as running,
/// and each one's unstarted cells read `queued` (they wait for one of the host's run slots).
#[test]
fn two_concurrent_running_batches_each_list_as_running_with_their_own_queued_cells() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, true);
    for (id, label) in [("b1", "exp-a"), ("b2", "exp-b")] {
        Batch::new(id, label)
            .tasks(&["gron"])
            .seeds(&[0, 1])
            .running(std::process::id())
            .on_this_host()
            .insert(&conn);
    }
    Run::new("r1", "exp-a", "gron", 0, "running")
        .at(1001.0)
        .in_batch("b1")
        .insert(&conn);
    let list = list_batches(dir.path()).unwrap();
    let running: Vec<&str> = list
        .iter()
        .filter(|b| b.status == BatchStatus::Running)
        .map(|b| b.id.as_str())
        .collect();
    assert_eq!(running.len(), 2, "both batches are running: {running:?}");
    for (id, queued) in [("b1", 1), ("b2", 2)] {
        let detail = batch_detail(dir.path(), id).unwrap().unwrap();
        assert_eq!(detail.status, BatchStatus::Running);
        assert_eq!(
            detail.cells.iter().filter(|c| c.state == "queued").count(),
            queued,
            "{id}"
        );
    }
}

/// A pb run's Zeroshot controller is inside pb's sandbox; whether the run is over is pb's
/// call: its own status, or its batch's runner having died.
#[test]
fn pb_says_whether_a_run_is_over() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert_eq!(
        pb_run_of_state_root(root, &root.join("runs/r1/agent/zeroshot-state")).as_deref(),
        Some("r1")
    );
    assert_eq!(
        pb_run_of_state_root(root, &root.join("runs/r1/agent")),
        None
    );
    assert_eq!(
        pb_run_of_state_root(root, Path::new("/elsewhere/runs/r1/agent/zeroshot-state")),
        None
    );

    let conn = seed_db(&root.join("pb.sqlite"), true);
    Batch::new("live", "x")
        .running(std::process::id())
        .insert(&conn);
    Batch::new("dead", "x").running(999_999_999).insert(&conn);
    Run::new("r-live", "x", "t", 0, "running")
        .in_batch("live")
        .insert(&conn);
    Run::new("r-orphan", "x", "t", 1, "running")
        .in_batch("dead")
        .insert(&conn);
    Run::new("r-stopped", "x", "t", 2, "interrupted")
        .in_batch("live")
        .insert(&conn);
    Run::new("r-label", "y", "t", 0, "running").insert(&conn);
    drop(conn);
    assert_eq!(pb_run_ended(root, "r-live"), Some(false));
    assert_eq!(
        pb_run_ended(root, "r-orphan"),
        Some(true),
        "its batch's runner is dead"
    );
    assert_eq!(pb_run_ended(root, "r-stopped"), Some(true));
    assert_eq!(
        pb_run_ended(root, "r-label"),
        Some(false),
        "no batch to reconcile: pb still calls it running"
    );
    assert_eq!(pb_run_ended(root, "nope"), None);
}
