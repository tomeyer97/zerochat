//! One batch's task × seed matrix: a cell per (task, seed) showing one of its attempts, and the
//! score aggregates per task (row), per seed (column) and overall.

use std::collections::HashMap;

use serde::Serialize;
use ts_rs::TS;

use super::batch::BatchStatus;
use super::db::{limits, ms};
use super::run::{RunRow, by_start_time, cell_state, cut_short};

/// A batch's runs by (task, seed) cell, each cell's attempts oldest first.
pub(super) type ByCell = HashMap<(String, i64), Vec<RunRow>>;

#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    pub task: String,
    pub seed: i64,
    pub run_id: String,
    pub zeroshot_run_id: Option<String>,
    /// `running_agent` | `evaluating` | `scored` | `interrupted` | `timeout` | `over_budget` |
    /// `crashed` | `failed` — see `cell_state`.
    #[ts(type = "BenchmarkCellState")]
    pub state: String,
    /// Set for `interrupted` | `timeout` | `over_budget` | `crashed` | `failed`.
    pub exit_reason: Option<String>,
    /// Set when `state` is `scored` but the harness itself hit a limit before pb evaluated it
    /// anyway (`eval_after_limit`): `timeout` or `budget`. The score is real and still counts as
    /// scored; this flag is what tells it apart from a run that finished on its own.
    pub over_limit: Option<String>,
    pub score: Option<f64>,
    pub tests_passed: Option<i64>,
    pub tests_total: Option<i64>,
    pub regression_passed: Option<i64>,
    pub regression_total: Option<i64>,
    pub cost_usd: Option<f64>,
    pub cap_usd: Option<f64>,
    pub wall_s: Option<f64>,
    /// The run's own wall-time and step caps (`limits_json`), so a cut-short cell can say which
    /// limit it hit, not just that it hit one.
    pub cap_minutes: Option<f64>,
    pub cap_steps: Option<i64>,
    /// When pb started this attempt (Unix ms): a running cell shows how long it has run.
    pub started_at: Option<i64>,
    /// Set when the cell is scored but its harness was cut short (see `cut_short`): the score
    /// is of partial work, and it IS in every mean (pb's own `compare` counts it too).
    #[ts(type = "CutShort | null")]
    pub cut_short: Option<String>,
    /// How many attempts pb recorded for this (task, seed) cell. 1 for almost every cell;
    /// greater than 1 when a task was retried (manually, or by a pseudo-batch that re-ran a
    /// failed seed) — the earlier attempts are never discarded, only not the one shown;
    /// `priorAttempts` lists them for a tooltip/link.
    pub attempts: usize,
    /// Every attempt other than the one shown, oldest first: enough to say "the 3rd attempt;
    /// 1 interrupted, 2 crashed" and link to each.
    pub prior_attempts: Vec<PriorAttempt>,
}

#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
pub struct PriorAttempt {
    pub run_id: String,
    pub zeroshot_run_id: Option<String>,
    #[ts(type = "BenchmarkCellState")]
    pub state: String,
    pub exit_reason: Option<String>,
    pub cost_usd: Option<f64>,
    pub wall_s: Option<f64>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Aggregate {
    pub key: String,
    pub mean: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub n: usize,
}

/// The mean, lowest and highest of some scores.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Stats {
    pub(super) mean: f64,
    pub(super) min: f64,
    pub(super) max: f64,
}

impl Stats {
    /// `None` for no scores.
    pub(super) fn of(scores: &[f64]) -> Option<Stats> {
        let (&first, rest) = scores.split_first()?;
        let (min, max) = rest
            .iter()
            .fold((first, first), |(min, max), &s| (min.min(s), max.max(s)));
        Some(Stats {
            mean: scores.iter().sum::<f64>() / scores.len() as f64,
            min,
            max,
        })
    }
}

impl Aggregate {
    fn of<'a>(key: String, cells: impl Iterator<Item = &'a Cell>) -> Aggregate {
        let scores: Vec<f64> = cells.filter_map(|c| c.score).collect();
        let stats = Stats::of(&scores);
        Aggregate {
            key,
            mean: stats.map(|s| s.mean),
            min: stats.map(|s| s.min),
            max: stats.map(|s| s.max),
            n: scores.len(),
        }
    }
}

/// Groups run rows by their (task, seed) cell, oldest attempt first. A cell normally has one
/// attempt; more than one means a retry (manual, or a pseudo-batch re-running a failed seed).
pub(super) fn group_attempts(runs: impl IntoIterator<Item = RunRow>) -> ByCell {
    let mut out = ByCell::new();
    for run in runs {
        out.entry((run.task_id.clone(), run.seed))
            .or_default()
            .push(run);
    }
    for attempts in out.values_mut() {
        attempts.sort_by(by_start_time);
    }
    out
}

/// Every attempt of every cell, in no particular order.
pub(super) fn all_attempts(by_cell: &ByCell) -> Vec<&RunRow> {
    by_cell.values().flatten().collect()
}

/// The one attempt a matrix cell shows: the still-running one if any attempt is live, else the
/// most recently scored attempt, else the most recent attempt at all (a terminal failure or
/// interruption with nothing to show instead). Earlier attempts are listed, never discarded —
/// see `Cell::attempts`/`prior_attempts`.
pub(super) fn pick_attempt(attempts: &[RunRow]) -> &RunRow {
    attempts
        .iter()
        .rev()
        .find(|r| r.status == "running")
        .or_else(|| attempts.iter().rev().find(|r| r.evaluated()))
        .or_else(|| attempts.last())
        .expect("group_attempts never creates an empty Vec")
}

impl Cell {
    /// A planned cell pb has not started (yet): a real batch knows its whole task × seed grid
    /// up front, but pb inserts a run's row only when a worker picks it up, so a missing row is
    /// `queued` while the batch runs and `not_started` once it ended (a Stop or a crash before
    /// pb got to it), not an empty cell.
    fn planned(task: &str, seed: i64, batch_status: &BatchStatus) -> Cell {
        Cell {
            task: task.to_owned(),
            seed,
            run_id: String::new(),
            zeroshot_run_id: None,
            state: if *batch_status == BatchStatus::Running {
                "queued"
            } else {
                "not_started"
            }
            .to_owned(),
            exit_reason: None,
            over_limit: None,
            score: None,
            tests_passed: None,
            tests_total: None,
            regression_passed: None,
            regression_total: None,
            cost_usd: None,
            cap_usd: None,
            wall_s: None,
            cap_minutes: None,
            cap_steps: None,
            started_at: None,
            cut_short: None,
            attempts: 0,
            prior_attempts: Vec::new(),
        }
    }

    /// The cell of a (task, seed) pb ran: it shows `pick_attempt`, and lists the others.
    fn from_attempts(task: &str, seed: i64, attempts: &[RunRow], batch_crashed: bool) -> Cell {
        let run = pick_attempt(attempts);
        let (state, exit_reason, over_limit) = cell_state(run, batch_crashed);
        let limits = limits(&run.limits_json).unwrap_or_default();
        Cell {
            task: task.to_owned(),
            seed,
            run_id: run.run_id.clone(),
            zeroshot_run_id: run.zeroshot_run_id.clone(),
            state: state.to_owned(),
            exit_reason,
            over_limit: over_limit.map(str::to_owned),
            score: run.score,
            tests_passed: run.tests_passed,
            tests_total: run.tests_total,
            regression_passed: run.regression_passed,
            regression_total: run.regression_total,
            cost_usd: run.cost_usd,
            cap_usd: limits.max_cost_usd,
            wall_s: run.wall_s,
            cap_minutes: limits.max_wall_minutes,
            cap_steps: limits.max_steps,
            started_at: (run.started_at > 0.0).then(|| ms(run.started_at)),
            cut_short: cut_short(run).map(str::to_owned),
            attempts: attempts.len(),
            prior_attempts: attempts
                .iter()
                .filter(|r| r.run_id != run.run_id)
                .map(|r| PriorAttempt::of(r, batch_crashed))
                .collect(),
        }
    }
}

impl PriorAttempt {
    fn of(run: &RunRow, batch_crashed: bool) -> PriorAttempt {
        let (state, exit_reason, _) = cell_state(run, batch_crashed);
        PriorAttempt {
            run_id: run.run_id.clone(),
            zeroshot_run_id: run.zeroshot_run_id.clone(),
            state: state.to_owned(),
            exit_reason,
            cost_usd: run.cost_usd,
            wall_s: run.wall_s,
        }
    }
}

pub(super) struct Matrix {
    /// Task-major: every seed of the first task, then of the next.
    pub(super) cells: Vec<Cell>,
    /// One per task, one per seed.
    pub(super) rows: Vec<Aggregate>,
    pub(super) cols: Vec<Aggregate>,
    /// Over every cell's score; `None` when no cell has one.
    pub(super) overall: Option<Stats>,
}

/// `planned`: a real batch's own status — every (task, seed) of its grid gets a cell, a missing
/// row reading `queued`/`not_started` (see `Cell::planned`). `None` for a label pseudo-batch,
/// whose grid is only what its runs happen to cover.
pub(super) fn build_matrix(
    tasks: &[String],
    seeds: &[i64],
    by_cell: &ByCell,
    batch_crashed: bool,
    planned: Option<&BatchStatus>,
) -> Matrix {
    let mut cells = Vec::new();
    for task in tasks {
        for &seed in seeds {
            match (by_cell.get(&(task.clone(), seed)), planned) {
                (Some(attempts), _) => {
                    cells.push(Cell::from_attempts(task, seed, attempts, batch_crashed))
                }
                (None, Some(status)) => cells.push(Cell::planned(task, seed, status)),
                (None, None) => {}
            }
        }
    }
    let rows = tasks
        .iter()
        .map(|t| Aggregate::of(t.clone(), cells.iter().filter(|c| &c.task == t)))
        .collect();
    let cols = seeds
        .iter()
        .map(|s| Aggregate::of(s.to_string(), cells.iter().filter(|c| c.seed == *s)))
        .collect();
    let scores: Vec<f64> = cells.iter().filter_map(|c| c.score).collect();
    Matrix {
        overall: Stats::of(&scores),
        cells,
        rows,
        cols,
    }
}
