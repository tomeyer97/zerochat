//! The batches list: one summary per real batch and per label pseudo-batch.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use ts_rs::TS;

use super::batch::{BatchRow, BatchStatus, HarnessIdentity, PseudoBatch, harness_identity};
use super::db::{json_len, ms, ms_opt, open_ro, query_runs};
use super::matrix::{ByCell, Stats, group_attempts, pick_attempt};
use super::run::cell_state;
use crate::pb_db::{has_column, has_table};

#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    pub id: String,
    pub label: Option<String>,
    /// The harness's own short version hash — what actually distinguishes two harnesses.
    pub harness_version: Option<String>,
    /// The Zeroshot CLI build this ran under (a tooltip detail, not the headline).
    pub zeroshot_version: Option<String>,
    /// The unique agent-node models the harness version's recorded runtime uses, sorted.
    pub models: Vec<String>,
    pub task_count: usize,
    pub seed_count: usize,
    /// Cells (task × seed), not run rows (a cell retried four times is still one cell): planned
    /// cells for a real batch (its whole grid, started or not), the cells its runs cover for a
    /// label pseudo-batch.
    pub runs_total: i64,
    /// Cells whose shown attempt reached a final state (scored, failed, over a limit,
    /// interrupted, crashed) — not running and not still being evaluated.
    pub runs_done: i64,
    /// Cells whose shown attempt is running or being evaluated: the list polls while any is.
    pub runs_in_flight: i64,
    /// Attempts beyond one per cell (a retry of an interrupted or crashed run), and what they
    /// cost: `total_cost_usd` includes it, the cell counts above do not.
    pub retried_attempts: i64,
    pub retried_cost_usd: Option<f64>,
    /// Over the shown attempt of each cell (the scores the matrix shows), not every attempt.
    pub mean_score: Option<f64>,
    pub min_score: Option<f64>,
    pub max_score: Option<f64>,
    /// Every attempt's spend, retried ones included.
    pub total_cost_usd: Option<f64>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub status: BatchStatus,
    /// A real `batches` row, not a pseudo-batch grouped by label.
    pub real: bool,
}

impl BatchSummary {
    /// The fields both kinds of batch fill the same way. `runs_total` is the cells its runs
    /// cover; the grid, the times and `real` are left for the caller.
    fn new(
        id: String,
        label: Option<String>,
        identity: HarnessIdentity,
        counts: CellCounts,
        status: BatchStatus,
    ) -> BatchSummary {
        BatchSummary {
            id,
            label,
            harness_version: identity.version,
            zeroshot_version: identity.zeroshot_version,
            models: identity.models,
            task_count: 0,
            seed_count: 0,
            runs_total: counts.cells,
            runs_done: counts.done,
            runs_in_flight: counts.in_flight,
            retried_attempts: counts.retried,
            retried_cost_usd: counts.retried_cost,
            mean_score: counts.scores.map(|s| s.mean),
            min_score: counts.scores.map(|s| s.min),
            max_score: counts.scores.map(|s| s.max),
            total_cost_usd: counts.total_cost,
            started_at: 0,
            ended_at: None,
            status,
            real: false,
        }
    }
}

/// One batch's runs counted the way its matrix shows them: per cell (its shown attempt, see
/// `pick_attempt`), with the attempts beyond one per cell (and their cost) counted apart.
#[derive(Default, Debug, PartialEq)]
pub(super) struct CellCounts {
    pub(super) cells: i64,
    pub(super) done: i64,
    pub(super) in_flight: i64,
    pub(super) retried: i64,
    pub(super) retried_cost: Option<f64>,
    /// Over each cell's shown score.
    pub(super) scores: Option<Stats>,
    pub(super) total_cost: Option<f64>,
}

pub(super) fn count_cells(by_cell: &ByCell, batch_crashed: bool) -> CellCounts {
    let mut c = CellCounts {
        cells: by_cell.len() as i64,
        ..Default::default()
    };
    let mut scores = Vec::new();
    for attempts in by_cell.values() {
        let shown = pick_attempt(attempts);
        match cell_state(shown, batch_crashed).0 {
            "running_agent" | "evaluating" => c.in_flight += 1,
            _ => c.done += 1,
        }
        scores.extend(shown.score);
        for run in attempts {
            if let Some(cost) = run.cost_usd {
                *c.total_cost.get_or_insert(0.0) += cost;
                if run.run_id != shown.run_id {
                    *c.retried_cost.get_or_insert(0.0) += cost;
                }
            }
        }
        c.retried += attempts.len() as i64 - 1;
    }
    c.scores = Stats::of(&scores);
    c
}

fn real_batch_summaries(conn: &Connection) -> Result<Vec<BatchSummary>> {
    let mut out = Vec::new();
    for batch in BatchRow::all(conn)? {
        let status = batch.status();
        let runs = query_runs(conn, "SELECT * FROM runs WHERE batch_id = ?1", [&batch.id])?;
        let counts = count_cells(&group_attempts(runs), status == BatchStatus::Crashed);
        let identity = harness_identity(conn, &batch.version_id);
        let (task_count, seed_count) = (json_len(&batch.task_ids), json_len(&batch.seeds));
        let cells = counts.cells;
        out.push(BatchSummary {
            task_count,
            seed_count,
            runs_total: ((task_count * seed_count) as i64).max(cells),
            started_at: ms(batch.started_at),
            ended_at: ms_opt(batch.ended_at),
            real: true,
            ..BatchSummary::new(batch.id, batch.label, identity, counts, status)
        });
    }
    Ok(out)
}

fn pseudo_batch_summaries(conn: &Connection, batch_col: bool) -> Result<Vec<BatchSummary>> {
    let mut out = Vec::new();
    for batch in PseudoBatch::all(conn, batch_col)? {
        let status = batch.status();
        let (id, started_at, ended_at) = (batch.id(), batch.started_at(), batch.ended_at());
        let (task_count, seed_count) = (batch.tasks().len(), batch.seeds().len());
        let identity = harness_identity(conn, &batch.version_id(conn, batch_col));
        let counts = count_cells(&group_attempts(batch.runs), status == BatchStatus::Crashed);
        out.push(BatchSummary {
            task_count,
            seed_count,
            started_at: ms(started_at),
            ended_at: ended_at.is_finite().then(|| ms(ended_at)),
            ..BatchSummary::new(id, batch.label, identity, counts, status)
        });
    }
    Ok(out)
}

/// Every batch: real `batches` rows (when the table exists) plus a pseudo-batch per `label`
/// among the runs with no `batch_id` (or every run, on a database with no `batches` table at
/// all), newest first.
pub fn list_batches(pb_root: &Path) -> Result<Vec<BatchSummary>> {
    let conn = open_ro(pb_root)?;
    let mut out = Vec::new();
    if has_table(&conn, "batches") {
        out.extend(real_batch_summaries(&conn)?);
    }
    let batch_col = has_column(&conn, "runs", "batch_id");
    out.extend(pseudo_batch_summaries(&conn, batch_col)?);
    out.sort_by_key(|a| std::cmp::Reverse(a.started_at));
    Ok(out)
}
