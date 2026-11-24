//! A replay batch's context: what it replayed, and how.

use super::super::*;
use super::{REPLAYS, Run, SWEEPS, migrate, seed_db, write_file};

/// A replay batch says what it replayed: the source run and node (and segment end), the model
/// the replays used vs the recorded one, the recorded score right after the segment (pb's own
/// `replays` baseline), what it started from, and its sweep.
#[test]
fn a_replay_batch_names_its_source_segment_models_and_recorded_score() {
    let dir = tempfile::tempdir().unwrap();
    let conn = seed_db(&dir.path().join("pb.sqlite"), false);
    migrate(&conn, SWEEPS);
    migrate(&conn, REPLAYS);
    // The source run: explore → build → triage → repair, on haiku; its score after each step.
    Run::new("src", "clone-v1", "gron", 0, "success")
        .scored(0.62)
        .cost(5.4)
        .at(100.0)
        .zeroshot_id("01zs-src")
        .insert(&conn);
    conn.execute_batch(
        "INSERT INTO node_executions (run_id, execution, node_id, node_name, model, cost_usd) VALUES
             ('src', 1, 'explore#1', 'explore', 'claude-haiku-4-5', 0.3),
             ('src', 2, 'build#1',   'build',   'claude-haiku-4-5', 0.9),
             ('src', 3, 'triage#1',  'triage',  'claude-haiku-4-5', 0.1),
             ('src', 4, 'repair#1',  'repair',  'claude-haiku-4-5', 0.4);
         INSERT INTO step_scores (run_id, seq, execution, node_id, score) VALUES
             ('src', 0, 1,    'explore#1', 0.0),
             ('src', 1, 2,    'build#1',   0.53),
             ('src', 2, 4,    'repair#1',  0.61),
             ('src', 3, NULL, 'final',     0.62);",
    )
    .unwrap();
    // Two replays of `build#1` on sonnet.
    let replay = |id, seed| {
        Run::new(id, "replay-build-sonnet", "gron", seed, "success")
            .model("claude-sonnet-5")
            .replay_of("src", "build#1", None)
    };
    replay("rp0", 0)
        .scored(0.88)
        .cost(5.8)
        .at(200.0)
        .zeroshot_id("01zs-rp0")
        .insert(&conn);
    replay("rp1", 1)
        .scored(0.82)
        .cost(5.2)
        .at(201.0)
        .zeroshot_id("01zs-rp1")
        .insert(&conn);
    conn.execute(
        "INSERT INTO node_executions (run_id, execution, node_id, node_name, model, cost_usd)
             VALUES ('rp0', 1, 'build#1', 'build', 'claude-sonnet-5', 1.4)",
        [],
    )
    .unwrap();
    // A sweep's replay of the segment triage#1 → repair#1.
    conn.execute(
        "INSERT INTO sweeps (sweep_id, created_at, source_run, spec_json, label) VALUES ('sw-1', 300.0, 'src', '{}', 'sweep-triage-repair')",
        [],
    )
    .unwrap();
    Run::new(
        "sw0",
        "sweep:sweep-triage-repair:triage#1:repair#1:claude-sonnet-5",
        "gron",
        0,
        "success",
    )
    .scored(0.61)
    .cost(2.2)
    .at(300.0)
    .zeroshot_id("01zs-sw0")
    .replay_of("src", "triage#1", Some("repair#1"))
    .in_sweep("sw-1")
    .insert(&conn);
    drop(conn);
    // rp1 has no node execution recorded yet: its model comes from its own runtime copy.
    write_file(
        dir.path(),
        "runs/rp1/harness/runtime.json",
        r#"{"nodes": {"build": {"kind": "agent", "model": "claude-sonnet-5"}}}"#,
    );
    write_file(
        dir.path(),
        "runs/rp1/harness/replay.json",
        r#"{"replay_of": "src", "node_id": "build#1", "through": null, "base_label": "pristine task workspace"}"#,
    );

    let detail = batch_detail(dir.path(), "label:replay-build-sonnet")
        .unwrap()
        .unwrap();
    assert_eq!(detail.replays.len(), 1);
    let r = &detail.replays[0];
    assert_eq!(
        (r.source_run.as_str(), r.node.as_str(), r.through.as_deref()),
        ("src", "build#1", None)
    );
    assert_eq!(r.source_zeroshot_run_id.as_deref(), Some("01zs-src"));
    assert_eq!(r.source_batch.as_deref(), Some("label:clone-v1"));
    assert_eq!(r.models, vec!["claude-sonnet-5".to_owned()]);
    assert_eq!(r.recorded_model.as_deref(), Some("claude-haiku-4-5"));
    assert_eq!(
        r.recorded_score,
        Some(0.53),
        "the source's score right after build#1, not its final 0.62"
    );
    assert_eq!(r.source_score, Some(0.62));
    assert_eq!(r.base.as_deref(), Some("pristine task workspace"));
    assert_eq!(r.runs, 2);

    let sweep = batch_detail(
        dir.path(),
        "label:sweep:sweep-triage-repair:triage#1:repair#1:claude-sonnet-5",
    )
    .unwrap()
    .unwrap();
    let s = &sweep.replays[0];
    assert_eq!(
        (s.node.as_str(), s.through.as_deref()),
        ("triage#1", Some("repair#1"))
    );
    assert_eq!(
        s.recorded_score,
        Some(0.61),
        "the score after the segment's last node"
    );
    assert_eq!(
        (s.sweep_id.as_deref(), s.sweep_label.as_deref()),
        (Some("sw-1"), Some("sweep-triage-repair"))
    );

    assert!(
        batch_detail(dir.path(), "label:clone-v1")
            .unwrap()
            .unwrap()
            .replays
            .is_empty(),
        "an ordinary batch has no replay context"
    );
}
