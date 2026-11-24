//! Two batches compared, per task and per node.

use std::path::Path;

use rusqlite::Connection;

use super::super::*;
use super::{Batch, NODE_DURATIONS, Run, migrate, node_exec, seed_db};

/// Batches `bA` (label `base`) and `bB` (label `cand`), node executions with their duration.
fn compare_db(path: &Path) -> Connection {
    let conn = seed_db(path, true);
    migrate(&conn, NODE_DURATIONS);
    Batch::new("bA", "base").insert(&conn);
    Batch::new("bB", "cand").insert(&conn);
    conn
}

/// A run of `bA` that finished (`success`); the test chains its score, cost and time.
fn run_a<'a>(id: &'a str, task: &'a str, seed: i64) -> Run<'a> {
    Run::new(id, "base", task, seed, "success").in_batch("bA")
}

/// The same in `bB`.
fn run_b<'a>(id: &'a str, task: &'a str, seed: i64) -> Run<'a> {
    Run::new(id, "cand", task, seed, "success").in_batch("bB")
}

#[test]
fn compare_detects_a_change_beyond_seed_spread() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    run_a("a1", "gron", 0)
        .scored(0.50)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_a("a2", "gron", 1)
        .scored(0.52)
        .cost(1.2)
        .wall(120.0)
        .insert(&conn);
    run_b("b1", "gron", 0)
        .scored(0.60)
        .cost(2.0)
        .wall(140.0)
        .insert(&conn);
    run_b("b2", "gron", 1)
        .scored(0.66)
        .cost(2.2)
        .wall(160.0)
        .insert(&conn);
    drop(conn);

    let cmp = compare_batches(dir.path(), "bA", "bB").unwrap();
    assert_eq!(cmp.a_label, Some("base".to_owned()));
    assert_eq!(cmp.b_label, Some("cand".to_owned()));
    assert_eq!(cmp.a_run_count, 2);
    assert_eq!(cmp.b_run_count, 2);
    let row = cmp.tasks.iter().find(|r| r.task == "gron").unwrap();
    assert_eq!(row.noise, Noise::BeyondSpread);
    assert!((row.delta_score.unwrap() - (0.63 - 0.51)).abs() < 1e-9);
    assert!((row.a.as_ref().unwrap().mean_cost_usd.unwrap() - 1.1).abs() < 1e-9);
    assert!((row.b.as_ref().unwrap().mean_time_s.unwrap() - 150.0).abs() < 1e-9);
    let totals = cmp.totals.unwrap();
    assert_eq!(totals.tasks, 1);
    assert!((totals.delta_score - row.delta_score.unwrap()).abs() < 1e-9);
}

#[test]
fn compare_marks_a_small_change_within_noise() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    run_a("a1", "t", 0)
        .scored(0.50)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_a("a2", "t", 1)
        .scored(0.60)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn); // spread 0.10
    run_b("b1", "t", 0)
        .scored(0.52)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_b("b2", "t", 1)
        .scored(0.54)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn); // spread 0.02
    drop(conn);

    let cmp = compare_batches(dir.path(), "bA", "bB").unwrap();
    let row = &cmp.tasks[0];
    // mean A = 0.55, mean B = 0.53, delta = -0.02; larger spread = 0.10 >= |delta|
    assert_eq!(row.noise, Noise::WithinNoise);
}

#[test]
fn compare_needs_seeds_with_only_one_scored_run_on_a_side() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    run_a("a1", "t", 0)
        .scored(0.50)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_b("b1", "t", 0)
        .scored(0.90)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_b("b2", "t", 1)
        .scored(0.92)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    drop(conn);

    let cmp = compare_batches(dir.path(), "bA", "bB").unwrap();
    assert_eq!(cmp.tasks[0].noise, Noise::NeedsSeeds);
}

#[test]
fn compare_marks_a_task_only_on_one_side_as_not_applicable() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    run_a("a1", "only-in-a", 0)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_b("b1", "only-in-b", 0)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    drop(conn);

    let cmp = compare_batches(dir.path(), "bA", "bB").unwrap();
    assert_eq!(cmp.tasks.len(), 2);
    assert!(cmp.tasks.iter().all(|r| r.noise == Noise::NotApplicable));
    assert!(cmp.totals.is_none());
}

#[test]
fn compare_unscored_runs_are_excluded_not_averaged_as_zero() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    Run::new("a1", "base", "t", 0, "running")
        .in_batch("bA")
        .insert(&conn);
    run_a("a2", "t", 1)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_a("a3", "t", 2)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_b("b1", "t", 0)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_b("b2", "t", 1)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    drop(conn);

    let cmp = compare_batches(dir.path(), "bA", "bB").unwrap();
    assert_eq!(cmp.a_run_count, 3); // the running run counts towards the batch...
    assert_eq!(cmp.tasks[0].a.as_ref().unwrap().n, 2); // ...but not towards the scored aggregate
}

#[test]
fn compare_reports_per_node_mean_cost_and_time_deltas() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    run_a("a1", "t", 0)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_b("b1", "t", 0)
        .scored(0.5)
        .cost(2.0)
        .wall(100.0)
        .insert(&conn);
    node_exec(&conn, "a1", "worker", 0.4, 10.0);
    node_exec(&conn, "b1", "worker", 0.9, 25.0);
    node_exec(&conn, "b1", "reviewer", 0.2, 5.0); // a node absent from A
    drop(conn);

    let cmp = compare_batches(dir.path(), "bA", "bB").unwrap();
    let worker = cmp.nodes.iter().find(|n| n.node == "worker").unwrap();
    assert!((worker.a.as_ref().unwrap().mean_cost_usd.unwrap() - 0.4).abs() < 1e-9);
    assert!((worker.delta_mean_cost_usd.unwrap() - 0.5).abs() < 1e-9);
    assert!((worker.delta_mean_time_s.unwrap() - 15.0).abs() < 1e-9);
    let reviewer = cmp.nodes.iter().find(|n| n.node == "reviewer").unwrap();
    assert!(reviewer.a.is_none() && reviewer.b.is_some());
    assert!(reviewer.delta_mean_cost_usd.is_none());
}

/// A ran `dupl` as well as the two shared tasks. The per-node table averages `build`'s
/// cost/time over the tasks both sides ran, not over every run of each batch (where A's extra,
/// cheaper `dupl` execution would drag A's mean cost down and its mean time up, showing a
/// task-mix difference as a cost difference).
#[test]
fn compare_nodes_is_restricted_to_shared_scored_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    // Shared tasks: gron, cheat. A-only: dupl (cheap and fast — would drag the naive mean
    // the wrong way if it were included).
    run_a("a-gron", "gron", 0)
        .scored(0.5)
        .cost(6.0)
        .wall(1800.0)
        .insert(&conn);
    run_a("a-cheat", "cheat", 0)
        .scored(0.5)
        .cost(6.6)
        .wall(1800.0)
        .insert(&conn);
    run_a("a-dupl", "dupl", 0)
        .scored(0.5)
        .cost(1.0)
        .wall(60.0)
        .insert(&conn);
    run_b("b-gron", "gron", 0)
        .scored(0.5)
        .cost(12.0)
        .wall(1600.0)
        .insert(&conn);
    run_b("b-cheat", "cheat", 0)
        .scored(0.5)
        .cost(12.74)
        .wall(1600.0)
        .insert(&conn);
    // A run still in flight on the shared task set: excluded from the node average too,
    // same as the task table excludes unscored runs (not averaged in as if cost were 0).
    Run::new("b-running", "cand", "gron", 1, "running")
        .in_batch("bB")
        .insert(&conn);
    for (run, cost) in [
        ("a-gron", 3.0),
        ("a-cheat", 3.08),
        ("a-dupl", 0.5),
        ("b-gron", 6.0),
        ("b-cheat", 6.37),
    ] {
        node_exec(&conn, run, "build", cost, 900.0);
    }
    node_exec(&conn, "b-running", "build", 0.01, 10.0); // partial cost of the still-running run: must not count
    drop(conn);

    let cmp = compare_batches(dir.path(), "bA", "bB").unwrap();
    let build = cmp.nodes.iter().find(|n| n.node == "build").unwrap();
    // Mean over the 2 shared-task scored executions on each side: A (3.00+3.08)/2 = 3.04, B
    // (6.00+6.37)/2 = 6.185 -> Δ +$3.145. Averaging in A's extra `dupl` run (and B's partial
    // still-running one) over "every run" would give a different, wrong number.
    assert!((build.a.as_ref().unwrap().mean_cost_usd.unwrap() - 3.04).abs() < 1e-9);
    assert!((build.b.as_ref().unwrap().mean_cost_usd.unwrap() - 6.185).abs() < 1e-9);
    assert!((build.delta_mean_cost_usd.unwrap() - 3.145).abs() < 1e-9);
}

#[test]
fn compare_nodes_has_no_rows_at_all_with_no_shared_task() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    run_a("a1", "only-a", 0)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    run_b("b1", "only-b", 0)
        .scored(0.5)
        .cost(1.0)
        .wall(100.0)
        .insert(&conn);
    node_exec(&conn, "a1", "worker", 0.4, 10.0);
    node_exec(&conn, "b1", "worker", 0.9, 25.0);
    drop(conn);

    let cmp = compare_batches(dir.path(), "bA", "bB").unwrap();
    assert!(
        cmp.nodes.is_empty(),
        "no shared task: nothing to compare per node either"
    );
}

#[test]
fn compare_works_against_label_pseudo_batches_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, false); // no batches table, and no node_executions.duration_s either
    Run::new("a1", "base", "t", 0, "success")
        .eval("ok", Some(0.5))
        .at(1000.0)
        .insert(&conn);
    Run::new("b1", "cand", "t", 0, "success")
        .eval("ok", Some(0.9))
        .at(1000.0)
        .insert(&conn);
    drop(conn);

    let cmp = compare_batches(dir.path(), "label:base", "label:cand").unwrap();
    assert_eq!(cmp.a_label, Some("base".to_owned()));
    assert_eq!(cmp.tasks[0].noise, Noise::NeedsSeeds); // one seed per side
    assert!((cmp.tasks[0].delta_score.unwrap() - 0.4).abs() < 1e-9);
}

#[test]
fn compare_of_two_unknown_batches_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let conn = compare_db(&dir.path().join("pb.sqlite"));
    drop(conn);
    let cmp = compare_batches(dir.path(), "no-such-a", "no-such-b").unwrap();
    assert_eq!(cmp.a_run_count, 0);
    assert_eq!(cmp.b_run_count, 0);
    assert!(cmp.tasks.is_empty());
}
