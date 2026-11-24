//! A cell's state, and a (task, seed) cell pb ran more than once.

use super::super::run::{RunRow, cell_state};
use super::super::*;
use super::{Run, seed_db};

// ---------- scored-over-a-limit and unscored-stop cells are not plain "failed" ----------

fn run_row(
    status: &str,
    eval_status: Option<&str>,
    score: Option<f64>,
    exit_reason: Option<&str>,
) -> RunRow {
    RunRow {
        status: status.to_owned(),
        eval_status: eval_status.map(str::to_owned),
        score,
        exit_reason: exit_reason.map(str::to_owned),
        ..Default::default()
    }
}

#[test]
fn cell_state_flags_a_scored_run_that_hit_a_limit_instead_of_hiding_it() {
    // pb evaluates the code a run produced even when the harness hit a cap
    // (`eval_after_limit`): still `scored` (it counts in every aggregate), but flagged.
    let (state, _, over) = cell_state(
        &run_row(
            "timeout",
            Some("ok"),
            Some(0.4),
            Some("wall-time limit 45 min"),
        ),
        false,
    );
    assert_eq!((state, over), ("scored", Some("timeout")));
    let (state, _, over) = cell_state(
        &run_row(
            "budget_exceeded",
            Some("ok"),
            Some(0.4),
            Some("cost limit $6.00"),
        ),
        false,
    );
    assert_eq!((state, over), ("scored", Some("budget")));
    // A clean success carries no flag.
    let (state, _, over) = cell_state(&run_row("success", Some("ok"), Some(0.9), None), false);
    assert_eq!((state, over), ("scored", None));
}

#[test]
fn cell_state_tells_a_stop_a_limit_and_a_crash_apart_from_a_plain_failure() {
    // An operator's own Stop with no score: `interrupted`, matching the Stop dialog's own
    // word, not a red "failed".
    let (state, reason, _) = cell_state(
        &run_row(
            "interrupted",
            Some("skipped"),
            None,
            Some("stopped by pb stop"),
        ),
        false,
    );
    assert_eq!(
        (state, reason.as_deref()),
        ("interrupted", Some("stopped by pb stop"))
    );
    // Still true when the harness died before pb's own runner marked eval_status at all.
    let (state, _, _) = cell_state(
        &run_row("interrupted", None, None, Some("stopped by pb stop")),
        false,
    );
    assert_eq!(state, "interrupted");
    // A limit hit with nothing scored: its own category, not lumped into "failed".
    let (state, _, _) = cell_state(
        &run_row(
            "timeout",
            Some("skipped"),
            None,
            Some("wall-time limit 45 min"),
        ),
        false,
    );
    assert_eq!(state, "timeout");
    let (state, _, _) = cell_state(
        &run_row("budget_exceeded", None, None, Some("cost limit $6.00")),
        false,
    );
    assert_eq!(state, "over_budget");
    // A harness crash: "crashed", not "failed".
    let (state, _, _) = cell_state(
        &run_row("crash", Some("skipped"), None, Some("pb process died")),
        false,
    );
    assert_eq!(state, "crashed");
    // pb's own evaluation erroring on a harness that DID succeed is still "failed" — that
    // part of the rule is unchanged.
    let (state, reason, _) = cell_state(&run_row("success", Some("error"), None, None), false);
    assert_eq!((state, reason.as_deref()), ("failed", Some("error")));
}

// ---------- survivorship: every attempt counts, one cell shows the live one ----------

#[test]
fn a_retried_cell_keeps_every_attempt_and_shows_the_scored_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, false);
    // Two failed attempts, then a scored one: the earlier two must not silently vanish (with
    // the list still saying every cell is done).
    Run::new("r1", "retry", "t", 0, "crash")
        .eval("skipped", None)
        .at(1000.0)
        .insert(&conn);
    Run::new("r2", "retry", "t", 0, "interrupted")
        .eval("skipped", None)
        .at(1100.0)
        .insert(&conn);
    Run::new("r3", "retry", "t", 0, "success")
        .eval("ok", Some(0.67))
        .at(1200.0)
        .insert(&conn);
    drop(conn);

    let detail = batch_detail(dir.path(), "label:retry").unwrap().unwrap();
    assert_eq!(
        detail.cells.len(),
        1,
        "one cell per (task, seed), not one row per attempt"
    );
    let cell = &detail.cells[0];
    assert_eq!(
        cell.run_id, "r3",
        "the scored attempt is shown, not whichever attempt happened to be inserted last"
    );
    assert_eq!(cell.score, Some(0.67));
    assert_eq!(cell.attempts, 3);
    assert_eq!(cell.prior_attempts.len(), 2);
    assert_eq!(cell.prior_attempts[0].run_id, "r1");
    assert_eq!(cell.prior_attempts[0].state, "crashed");
    assert_eq!(cell.prior_attempts[1].run_id, "r2");
    assert_eq!(cell.prior_attempts[1].state, "interrupted");
    // The retries are counted apart, with their cost, and the total includes them.
    assert_eq!(detail.retried_attempts, 2);
    assert!((detail.retried_cost_usd.unwrap() - 0.84).abs() < 1e-9);
    assert!((detail.total_cost_usd.unwrap() - 1.26).abs() < 1e-9);

    // The list counts cells (1/1 done), not run rows ("3/3 runs" for a 1×1), and its mean is
    // over the score each cell shows.
    let summary = list_batches(dir.path())
        .unwrap()
        .into_iter()
        .find(|b| b.id == "label:retry")
        .unwrap();
    assert_eq!(
        (
            summary.runs_done,
            summary.runs_total,
            summary.runs_in_flight
        ),
        (1, 1, 0)
    );
    assert_eq!(summary.retried_attempts, 2);
    assert!((summary.retried_cost_usd.unwrap() - 0.84).abs() < 1e-9);
    assert!((summary.total_cost_usd.unwrap() - 1.26).abs() < 1e-9);
    assert_eq!(summary.mean_score, Some(0.67));
    assert_eq!(summary.status, BatchStatus::Done);
}

#[test]
fn a_cell_with_only_failed_attempts_shows_the_most_recent_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pb.sqlite");
    let conn = seed_db(&path, false);
    Run::new("r1", "allfail", "t", 0, "crash")
        .eval("skipped", None)
        .at(1000.0)
        .insert(&conn);
    Run::new("r2", "allfail", "t", 0, "crash")
        .eval("skipped", None)
        .at(1100.0)
        .insert(&conn);
    drop(conn);

    let detail = batch_detail(dir.path(), "label:allfail").unwrap().unwrap();
    let cell = &detail.cells[0];
    assert_eq!(cell.run_id, "r2");
    assert_eq!(cell.attempts, 2);
    assert_eq!(cell.prior_attempts.len(), 1);
    assert_eq!(cell.prior_attempts[0].run_id, "r1");
}
