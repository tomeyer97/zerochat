//! One run's score card.

use super::super::*;
use super::{Run, seed_db};

#[test]
fn run_score_is_found_by_zeroshot_run_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, false);
    Run::new("r1", "baseline", "gron", 1, "success")
        .eval("ok", Some(0.85))
        .at(1000.0)
        .insert(&conn);
    conn.execute_batch(
        "INSERT INTO test_results (run_id, branch, name, status)
             VALUES ('r1', 'unit', 'test_a', 'passed'), ('r1', 'unit', 'test_b', 'failed');
         INSERT INTO step_scores (run_id, seq, node_id, score, tests_passed, tests_total)
             VALUES ('r1', 1, 'worker#1', 0.4, 4, 10), ('r1', 2, 'repair#1', 0.85, 8, 10);
         INSERT INTO node_executions (run_id, node_name, cost_usd)
             VALUES ('r1', 'worker', 0.2), ('r1', 'worker', 0.1), ('r1', 'repair', 0.12);",
    )
    .unwrap();
    drop(conn);

    let score = run_score(dir.path(), "zs-r1").unwrap().unwrap();
    assert_eq!(score.run_id, "r1");
    assert_eq!(score.score, Some(0.85));
    assert_eq!(score.failing_tests, vec!["unit/test_b".to_owned()]);
    assert_eq!(score.steps.len(), 2);
    assert_eq!(score.cap_usd, Some(5.0));
    let worker_cost = score
        .node_costs
        .iter()
        .find(|n| n.node == "worker")
        .unwrap();
    assert!((worker_cost.cost_usd - 0.3).abs() < 1e-9);
    assert_eq!(worker_cost.calls, 2);

    // Found by pb's own run_id too.
    assert_eq!(run_score(dir.path(), "r1").unwrap().unwrap().run_id, "r1");
    assert!(run_score(dir.path(), "no-such-run").unwrap().is_none());
}
