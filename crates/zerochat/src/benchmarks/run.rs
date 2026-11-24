//! One pb run: its `runs` row, what its harness's `status` means, the state its matrix cell
//! shows, and whether pb sees it as over.

use std::cmp::Ordering;
use std::path::Path;

use rusqlite::Row;

use super::batch::{BatchStatus, reconciled_status};
use super::db::{get_opt, open_ro, query_run};
use crate::paths;

/// One `runs` row, with whatever columns this database has.
#[derive(Debug, Clone, Default)]
pub(super) struct RunRow {
    pub(super) run_id: String,
    pub(super) label: Option<String>,
    pub(super) task_id: String,
    pub(super) seed: i64,
    /// How the harness ended, as pb wrote it: see `HarnessOutcome`.
    pub(super) status: String,
    pub(super) exit_reason: Option<String>,
    pub(super) zeroshot_run_id: Option<String>,
    pub(super) started_at: f64,
    pub(super) ended_at: Option<f64>,
    pub(super) wall_s: Option<f64>,
    pub(super) cost_usd: Option<f64>,
    pub(super) reported_cost_usd: Option<f64>,
    pub(super) score: Option<f64>,
    pub(super) tests_passed: Option<i64>,
    pub(super) tests_total: Option<i64>,
    /// pb's evaluation of the run's code: absent or `pending` until it ran, `ok` with a score,
    /// else what went wrong (`skipped`, `error`, ...).
    pub(super) eval_status: Option<String>,
    pub(super) eval_error: Option<String>,
    pub(super) regression_passed: Option<i64>,
    pub(super) regression_total: Option<i64>,
    pub(super) limits_json: Option<String>,
    pub(super) batch_id: Option<String>,
    pub(super) model_override: Option<String>,
    /// `pb replay` / `pb sweep`: the recorded run this one replays, from which node, through
    /// which, and the sweep it belongs to.
    pub(super) replay_of: Option<String>,
    pub(super) replay_node: Option<String>,
    pub(super) replay_through: Option<String>,
    pub(super) sweep_id: Option<String>,
}

pub(super) fn row_to_run(row: &Row) -> RunRow {
    RunRow {
        // Required columns: never NULL in pb's schema.
        run_id: get_opt(row, "run_id").unwrap_or_default(),
        task_id: get_opt(row, "task_id").unwrap_or_default(),
        seed: get_opt(row, "seed").unwrap_or(0),
        status: get_opt(row, "status").unwrap_or_default(),
        started_at: get_opt(row, "started_at").unwrap_or(0.0),
        // Optional columns: NULL, or not in this database's schema.
        label: get_opt(row, "label"),
        exit_reason: get_opt(row, "exit_reason"),
        zeroshot_run_id: get_opt(row, "zeroshot_run_id"),
        ended_at: get_opt(row, "ended_at"),
        wall_s: get_opt(row, "wall_s"),
        cost_usd: get_opt(row, "cost_usd"),
        reported_cost_usd: get_opt(row, "reported_cost_usd"),
        score: get_opt(row, "score"),
        tests_passed: get_opt(row, "tests_passed"),
        tests_total: get_opt(row, "tests_total"),
        eval_status: get_opt(row, "eval_status"),
        eval_error: get_opt(row, "eval_error"),
        regression_passed: get_opt(row, "regression_passed"),
        regression_total: get_opt(row, "regression_total"),
        limits_json: get_opt(row, "limits_json"),
        batch_id: get_opt(row, "batch_id"),
        model_override: get_opt(row, "model_override"),
        replay_of: get_opt(row, "replay_of"),
        replay_node: get_opt(row, "replay_node"),
        replay_through: get_opt(row, "replay_through"),
        sweep_id: get_opt(row, "sweep_id"),
    }
}

impl RunRow {
    pub(super) fn outcome(&self) -> HarnessOutcome {
        HarnessOutcome::of(&self.status)
    }

    /// pb evaluated the run's code and has a score for it.
    pub(super) fn evaluated(&self) -> bool {
        self.eval_status.as_deref() == Some("ok")
    }
}

/// Oldest first.
pub(super) fn by_start_time(a: &RunRow, b: &RunRow) -> Ordering {
    a.started_at
        .partial_cmp(&b.started_at)
        .unwrap_or(Ordering::Equal)
}

/// How a run's harness ended: pb's `runs.status`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HarnessOutcome {
    Running,
    Success,
    /// A person stopped it (`pb stop`, Ctrl-C).
    Interrupted,
    /// It hit its wall-time cap.
    Timeout,
    /// It hit its cost cap.
    BudgetExceeded,
    /// It hit its step cap.
    StepLimit,
    /// The harness process died (`crash`, or `crashed`).
    Crashed,
    /// Any other status.
    Failed,
}

impl HarnessOutcome {
    pub(super) fn of(status: &str) -> Self {
        match status {
            "running" => Self::Running,
            "success" => Self::Success,
            "interrupted" => Self::Interrupted,
            "timeout" => Self::Timeout,
            "budget_exceeded" => Self::BudgetExceeded,
            "step_limit" => Self::StepLimit,
            "crash" | "crashed" => Self::Crashed,
            _ => Self::Failed,
        }
    }

    /// The cap it hit: `timeout` | `budget` | `steps`.
    fn limit(self) -> Option<&'static str> {
        match self {
            Self::Timeout => Some("timeout"),
            Self::BudgetExceeded => Some("budget"),
            Self::StepLimit => Some("steps"),
            _ => None,
        }
    }

    /// The cell state of a run that ended this way with no score.
    fn unscored_state(self) -> &'static str {
        match self {
            Self::Interrupted => "interrupted",
            Self::Timeout => "timeout",
            Self::BudgetExceeded => "over_budget",
            Self::StepLimit => "over_steps",
            Self::Crashed => "crashed",
            Self::Running | Self::Success | Self::Failed => "failed",
        }
    }

    /// Why the harness of a scored run did not finish on its own: a cap, a Stop, or anything
    /// else that ended it early (`crashed`).
    fn cut_short(self) -> Option<&'static str> {
        match self {
            Self::Running | Self::Success => None,
            Self::Interrupted => Some("interrupted"),
            Self::Crashed | Self::Failed => Some("crashed"),
            Self::Timeout | Self::BudgetExceeded | Self::StepLimit => self.limit(),
        }
    }
}

/// The state a run's matrix cell shows, with the reason for a run that ended without a score,
/// and the cap a scored run hit (`over_limit`: `timeout` | `budget` | `steps`):
///
/// - `running_agent`: Zeroshot is still running the agent.
/// - `crashed`: still `running` in pb.sqlite, but the batch's pb process died (`batch_crashed`),
///   so nothing will ever finish the row; or the harness process itself died.
/// - `evaluating`: the harness finished and pb's hidden-test score is not in yet (this can
///   take 3-25 minutes).
/// - `scored`: pb evaluated the run. pb also evaluates the code a run produced when its harness
///   hit a cap (`eval_after_limit`): still `scored`, and it counts as scored everywhere, with
///   `over_limit` saying which cap.
/// - `interrupted`, `timeout`, `over_budget`, `over_steps`: the harness was stopped by a person
///   or a cap, with no score.
/// - `failed`: anything else, with the reason pb recorded (the harness's `exit_reason`, or
///   `eval_error` when the harness succeeded and pb's own evaluation failed).
pub(super) fn cell_state(
    run: &RunRow,
    batch_crashed: bool,
) -> (&'static str, Option<String>, Option<&'static str>) {
    let outcome = run.outcome();
    if outcome == HarnessOutcome::Running {
        if batch_crashed {
            return (
                "crashed",
                Some("pb's process died before finishing this run".to_owned()),
                None,
            );
        }
        return ("running_agent", None, None);
    }
    if outcome != HarnessOutcome::Success {
        // Classified by the harness's outcome whatever `eval_status` says: a harness that died
        // before pb's runner could mark the row `skipped` must not read `evaluating` forever.
        if run.evaluated() {
            return ("scored", None, outcome.limit());
        }
        let reason = run
            .exit_reason
            .clone()
            .unwrap_or_else(|| run.status.clone());
        return (outcome.unscored_state(), Some(reason), None);
    }
    match run.eval_status.as_deref() {
        None | Some("pending") => ("evaluating", None, None),
        Some("ok") => ("scored", None, None),
        eval => {
            let reason = run
                .eval_error
                .clone()
                .or_else(|| eval.map(str::to_owned))
                .unwrap_or_else(|| "eval failed".to_owned());
            ("failed", Some(reason), None)
        }
    }
}

/// Why a *scored* run's harness did not finish on its own, or `None` when it did: pb scores the
/// code that existed when a cap, a Stop or a crash ended the agent (`eval_after_limit`, and
/// `do_eval` in pb's runner for a crash), so the score is real but measures partial work.
/// `timeout` | `budget` | `steps` | `interrupted` | `crashed`.
pub(super) fn cut_short(run: &RunRow) -> Option<&'static str> {
    if run.score.is_none() || !run.evaluated() {
        return None;
    }
    run.outcome().cut_short()
}

/// The pb run a Zeroshot state root belongs to: `<pb_root>/runs/<run_id>/agent/zeroshot-state`
/// (`paths::pb_run_state_root`, the `pb` source's roots). `None` for any other directory.
pub fn pb_run_of_state_root(pb_root: &Path, state_root: &Path) -> Option<String> {
    let rest = state_root.strip_prefix(pb_root.join("runs")).ok()?;
    let run_id = rest.components().next()?.as_os_str().to_str()?;
    (state_root == paths::pb_run_state_root(pb_root, run_id)).then(|| run_id.to_owned())
}

/// Whether a pb run has ended as pb itself sees it: `status` is no longer `running`, or the
/// batch it ran in has no live runner any more (`reconciled_status`). A pb run whose Zeroshot
/// ledger still says "running" after pb recorded it interrupted or crashed (pb tore the sandbox
/// down: the ledger never got its terminal event) really has no controller; one pb still calls
/// running has a controller zerochat cannot see, inside pb's sandbox. `None` when pb.sqlite has
/// no such run (or cannot be read).
pub fn pb_run_ended(pb_root: &Path, run_id: &str) -> Option<bool> {
    let conn = open_ro(pb_root).ok()?;
    let run = query_run(&conn, "SELECT * FROM runs WHERE run_id = ?1", [run_id])?;
    if run.outcome() != HarnessOutcome::Running {
        return Some(true);
    }
    let batch = run.batch_id.and_then(|b| {
        conn.query_row(
            "SELECT status, pid, host FROM batches WHERE id = ?1",
            [b],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .ok()
    });
    Some(batch.is_some_and(|(status, pid, host)| {
        reconciled_status(&status, pid, host.as_deref()) != BatchStatus::Running
    }))
}
