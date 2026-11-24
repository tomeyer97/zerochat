//! The sweeps list.

use super::super::*;
use super::{Run, SWEEPS, migrate, seed_db};

#[test]
fn list_sweeps_is_empty_without_a_sweeps_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    seed_db(&path, false);
    assert!(list_sweeps(dir.path()).unwrap().is_empty());
}

#[test]
fn list_sweeps_aggregates_runs_and_dedups_models_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, true);
    migrate(&conn, SWEEPS);
    Run::new("src", "x", "gron", 0, "success")
        .at(900.0)
        .insert(&conn);
    let spec = serde_json::json!({"n": 2, "items": [
        {"item": "build#1", "models": ["haiku", "sonnet"]},
        {"item": "triage#1:repair#1", "models": ["sonnet", "opus"]},
    ]});
    conn.execute(
        "INSERT INTO sweeps (sweep_id, created_at, source_run, spec_json, label) VALUES ('sw-1', 2000.0, 'src', ?1, 'L')",
        [spec.to_string()],
    )
    .unwrap();
    let replays = [
        ("rep0", "success", 1.0),
        ("rep1", "success", 1.5),
        ("rep2", "running", 0.5),
    ];
    for (seed, (id, status, cost)) in replays.into_iter().enumerate() {
        Run::new(id, "r", "gron", seed as i64, status)
            .at(901.0)
            .cost(cost)
            .in_sweep("sw-1")
            .insert(&conn);
    }
    drop(conn);

    let sweeps = list_sweeps(dir.path()).unwrap();
    assert_eq!(sweeps.len(), 1);
    let s = &sweeps[0];
    assert_eq!(s.sweep_id, "sw-1");
    assert_eq!(s.source_run, "src");
    assert_eq!(s.task_id, Some("gron".to_owned()));
    assert_eq!(
        s.items,
        vec!["build#1".to_owned(), "triage#1:repair#1".to_owned()]
    );
    assert_eq!(
        s.models,
        vec!["haiku".to_owned(), "sonnet".to_owned(), "opus".to_owned()]
    ); // first-seen order, deduped
    assert_eq!(s.n, 2);
    assert_eq!(s.runs, 3);
    assert_eq!(s.running, 1);
    assert!((s.cost_usd.unwrap() - 3.0).abs() < 1e-9);
}
