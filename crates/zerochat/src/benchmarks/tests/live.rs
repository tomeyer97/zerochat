//! Batches pb has not recorded yet, and what a running cell's agent is doing and has spent.

use std::time::{Duration, SystemTime};

use rusqlite::Connection;

use super::super::live::START_GRACE;
use super::super::pb_pricing::{CallTokens, pb_pricing, rates_for};
use super::super::*;
use super::{Batch, Run, seed_db, write_file};

/// `pb run --detach` prints the id before its runner has written the `batches` row. Until it
/// does, the id reads `starting` (not 404); once the runner is gone without a row,
/// `failed_to_start` with pb's own log saying why. An id nobody started is still `None`.
#[test]
fn a_batch_id_with_no_row_yet_is_starting_then_failed_to_start() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    assert!(
        pending_batch(dir.path(), "20261107-120000-abcdef", None, now).is_none(),
        "nobody started it"
    );
    assert!(
        pending_batch(
            dir.path(),
            "../etc",
            Some(StartedHere { at: now, pid: None }),
            now
        )
        .is_none()
    );

    let alive = StartedHere {
        at: now,
        pid: Some(std::process::id()),
    };
    let p = pending_batch(dir.path(), "20261107-120000-abcdef", Some(alive), now).unwrap();
    assert_eq!((&p.status, p.starting), (&BatchStatus::Starting, true));
    assert!(p.started_at.is_some() && p.cells.is_empty());

    // pb's parent creates batches/<id>/pb.log before spawning the runner; the runner dies.
    write_file(
        dir.path(),
        "batches/20261107-120000-abcdef/pb.log",
        "Traceback (most recent call last):\n\nKeyError: 'claude_bin'\n",
    );
    let dead = StartedHere {
        at: now,
        pid: Some(999_999_999),
    };
    let p = pending_batch(dir.path(), "20261107-120000-abcdef", Some(dead), now).unwrap();
    assert_eq!(
        (&p.status, p.starting),
        (&BatchStatus::FailedToStart, false)
    );
    assert_eq!(
        p.start_log.as_deref(),
        Some("Traceback (most recent call last):\nKeyError: 'claude_bin'")
    );

    // After a restart zerochat no longer remembers starting it: the directory alone says
    // pb did; silence past the grace period reads as failed.
    let p = pending_batch(dir.path(), "20261107-120000-abcdef", None, now).unwrap();
    assert_eq!(p.status, BatchStatus::Starting);
    let later = now + START_GRACE + Duration::from_secs(5);
    assert_eq!(
        pending_batch(dir.path(), "20261107-120000-abcdef", None, later)
            .unwrap()
            .status,
        BatchStatus::FailedToStart
    );
}

/// A real batch knows its whole grid; a cell pb has not started is `queued` while the batch
/// runs (and `not_started` once it ended), not the same "—" as an empty cell. A running cell
/// carries its start time, and its Zeroshot run id is found from the run's own state directory
/// when pb has not written it yet.
#[test]
fn a_running_batch_shows_queued_cells_and_links_a_running_cell_to_its_live_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, true);
    Batch::new("b1", "live")
        .tasks(&["gron"])
        .seeds(&[0, 1, 2])
        .parallel(1)
        .running(std::process::id())
        .insert(&conn);
    Run::new("r0", "live", "gron", 0, "success")
        .scored(0.8)
        .cost(3.0)
        .wall(900.0)
        .in_batch("b1")
        .insert(&conn);
    Run::new("r1", "live", "gron", 1, "running")
        .at(1700000000.5)
        .no_zeroshot_id()
        .in_batch("b1")
        .insert(&conn);
    drop(conn);
    write_file(
        dir.path(),
        "runs/r1/agent/zeroshot-state/runs/01zs-live/runs.sqlite3",
        "",
    );

    let detail = batch_detail(dir.path(), "b1").unwrap().unwrap();
    assert_eq!(detail.status, BatchStatus::Running);
    assert_eq!(detail.cells.len(), 3, "every planned cell, started or not");
    let running = detail.cells.iter().find(|c| c.seed == 1).unwrap();
    assert_eq!(running.state, "running_agent");
    assert_eq!(running.started_at, Some(1_700_000_000_500));
    assert_eq!(running.zeroshot_run_id, None);
    let queued = detail.cells.iter().find(|c| c.seed == 2).unwrap();
    assert_eq!(
        (
            queued.state.as_str(),
            queued.run_id.as_str(),
            queued.attempts
        ),
        ("queued", "", 0)
    );

    let mut value = serde_json::to_value(&detail).unwrap();
    fill_live_cells(dir.path(), &mut value);
    let cells = value["cells"].as_array().unwrap();
    assert_eq!(
        cells.iter().find(|c| c["seed"] == 1).unwrap()["zeroshotRunId"],
        "01zs-live"
    );
    assert_eq!(
        cells.iter().find(|c| c["seed"] == 0).unwrap()["zeroshotRunId"],
        "zs-r0",
        "a recorded id is kept"
    );

    // Once the batch ended (a Stop before pb got to seed 2), the same gap is `not_started`.
    let conn = Connection::open(&path).unwrap();
    conn.execute("UPDATE batches SET status='stopped' WHERE id='b1'", [])
        .unwrap();
    conn.execute("UPDATE runs SET status='interrupted' WHERE run_id='r1'", [])
        .unwrap();
    drop(conn);
    let detail = batch_detail(dir.path(), "b1").unwrap().unwrap();
    assert_eq!(
        detail.cells.iter().find(|c| c.seed == 2).unwrap().state,
        "not_started"
    );
}

/// A running cell says which node its agent is in (Zeroshot's own ledger) and what it has spent
/// so far (the shim's live.json, priced with pb.toml like pb's runner).
#[test]
fn a_running_cell_carries_its_current_node_and_live_spend() {
    let dir = tempfile::tempdir().unwrap();
    let conn = seed_db(&dir.path().join("pb.sqlite"), true);
    Batch::new("b1", "live")
        .tasks(&["gron"])
        .seeds(&[0])
        .parallel(1)
        .running(std::process::id())
        .insert(&conn);
    Run::new("r1", "live", "gron", 0, "running")
        .no_zeroshot_id()
        .in_batch("b1")
        .insert(&conn);
    drop(conn);
    write_file(
        dir.path(),
        "pb.toml",
        r#"
        [pricing."claude-haiku-4-5"]
        input = 1.0
        output = 5.0
        cache_read = 0.1
        cache_write = 1.25
        cache_write_1h = 2.0
        [pricing."claude-haiku"]
        input = 99.0
        output = 99.0
        cache_read = 99.0
        cache_write = 99.0
        "#,
    );
    let ledger_dir = dir
        .path()
        .join("runs/r1/agent/zeroshot-state/runs/01zs-live");
    std::fs::create_dir_all(&ledger_dir).unwrap();
    let ledger = Connection::open(ledger_dir.join("runs.sqlite3")).unwrap();
    ledger
        .execute_batch(
            "CREATE TABLE v2_run_events (sequence INTEGER PRIMARY KEY, event_json TEXT NOT NULL);",
        )
        .unwrap();
    for (seq, event) in [
        (1, r#"{"kind":"run_started"}"#),
        (
            2,
            r#"{"kind":"node_started","reference":{"node":"explore","nodeInstance":1,"execution":1}}"#,
        ),
        (
            3,
            r#"{"kind":"node_completed","completion":{"reference":{"node":"explore","nodeInstance":1,"execution":1}}}"#,
        ),
        (
            4,
            r#"{"kind":"node_started","reference":{"node":"build","nodeInstance":2,"execution":2}}"#,
        ),
    ] {
        ledger
            .execute(
                "INSERT INTO v2_run_events VALUES (?1, ?2)",
                rusqlite::params![seq, event],
            )
            .unwrap();
    }
    write_file(
        dir.path(),
        "runs/r1/agent/calls/c1/live.json",
        r#"{"messages": {
            "m1": ["claude-haiku-4-5-20251001", {"input_tokens": 10, "output_tokens": 100, "cache_read_input_tokens": 1000,
                    "cache_creation_input_tokens": 2000, "cache_creation": {"ephemeral_1h_input_tokens": 1500}}, null],
            "m2": ["<synthetic>", {"input_tokens": 999999}, null],
            "m3": ["gpt-x", {"input_tokens": 5}, null]
        }, "ts": 1}"#,
    );
    std::fs::create_dir_all(dir.path().join("runs/r1/agent/calls/c2-no-live-yet")).unwrap();

    let mut value = serde_json::to_value(batch_detail(dir.path(), "b1").unwrap().unwrap()).unwrap();
    fill_live_cells(dir.path(), &mut value);
    let cell = &value["cells"][0];
    assert_eq!(cell["zeroshotRunId"], "01zs-live");
    assert_eq!(cell["liveNodes"], serde_json::json!(["build"]));
    // (10×1 + 100×5 + 1000×0.1 + 500×1.25 + 1500×2.0) / 1e6, the longest matching price key.
    assert!(
        (cell["liveCostUsd"].as_f64().unwrap() - 4235e-6).abs() < 1e-12,
        "{}",
        cell["liveCostUsd"]
    );
    assert_eq!(cell["liveCostUnpriced"], serde_json::json!(["gpt-x"]));

    ledger.execute("INSERT INTO v2_run_events VALUES (5, '{\"kind\":\"terminal\",\"result\":{\"status\":\"succeeded\"}}')", []).unwrap();
    let mut value = serde_json::to_value(batch_detail(dir.path(), "b1").unwrap().unwrap()).unwrap();
    fill_live_cells(dir.path(), &mut value);
    assert_eq!(
        value["cells"][0]["liveNodes"],
        serde_json::json!([]),
        "the agent finished; pb is about to evaluate"
    );
}

/// Like pb's `cost_of`: with no `cache_write_1h` in pb.toml, a 1-hour cache write costs 1.6×
/// the 5-minute price; prices may be written as integers.
#[test]
fn pb_pricing_defaults_the_1h_cache_write_price_and_accepts_integers() {
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "pb.toml",
        "[pricing.\"m\"]\ninput = 1\noutput = 2\ncache_read = 0.5\ncache_write = 10\n[pricing.\"incomplete\"]\ninput = 1\n",
    );
    let pricing = pb_pricing(dir.path());
    assert!(
        rates_for("incomplete", &pricing).is_none(),
        "a model missing a price is left out"
    );
    let rates = rates_for("m-20250101", &pricing).unwrap();
    let one_million = |set: fn(&mut CallTokens)| {
        let mut tokens = CallTokens::default();
        set(&mut tokens);
        rates.cost(&tokens)
    };
    assert_eq!(one_million(|t| t.input = 1e6), 1.0);
    assert_eq!(one_million(|t| t.output = 1e6), 2.0);
    assert_eq!(one_million(|t| t.cache_write_5m = 1e6), 10.0);
    assert!((one_million(|t| t.cache_write_1h = 1e6) - 16.0).abs() < 1e-9);
}
