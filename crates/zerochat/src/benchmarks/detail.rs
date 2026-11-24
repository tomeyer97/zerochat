//! One batch's page: its header (harness, caps, times, status), its matrix and totals, the
//! harnesses it ran and what it replayed.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use ts_rs::TS;

use super::batch::{BatchRow, BatchStatus, PseudoBatch, harness_identity};
use super::db::{Limits, limits, ms, ms_opt, open_ro, query_runs};
use super::list::count_cells;
use super::live::start_log_tail;
use super::matrix::{Aggregate, ByCell, Cell, all_attempts, build_matrix, group_attempts};
use super::replays::{ReplayContext, replay_contexts};
use super::run::{RunRow, by_start_time};
use crate::pb_db::has_column;

#[derive(Serialize, Default, TS)]
#[serde(rename_all = "camelCase")]
pub struct BatchDetail {
    pub id: String,
    /// pb has not started any run of this batch yet — either its `batches` row exists
    /// with no runs (pb is preparing its first sandbox), or (`status` `starting`) the id `pb run
    /// --detach` printed has no row at all yet. The page keeps polling instead of saying "not
    /// found".
    pub starting: bool,
    /// The tail of pb's own log for a batch that is starting or failed to start
    /// (`<pb_root>/batches/<id>/pb.log`, what the detached runner prints).
    pub start_log: Option<String>,
    pub label: Option<String>,
    pub harness_version: Option<String>,
    pub zeroshot_version: Option<String>,
    pub models: Vec<String>,
    pub limits: Option<Limits>,
    /// Real batches only (`batches.parallel`); `None` for a label pseudo-batch.
    pub parallel: Option<i64>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub status: BatchStatus,
    pub tasks: Vec<String>,
    pub seeds: Vec<i64>,
    pub cells: Vec<Cell>,
    pub row_aggregates: Vec<Aggregate>,
    pub col_aggregates: Vec<Aggregate>,
    pub mean_score: Option<f64>,
    pub min_score: Option<f64>,
    pub max_score: Option<f64>,
    /// Summed over every attempt (not just the cell shown), so a retried/interrupted attempt's
    /// spend is never invisible.
    pub total_cost_usd: Option<f64>,
    pub total_wall_s: Option<f64>,
    /// Attempts beyond one per cell (retries of interrupted or crashed runs) and
    /// their cost — part of `total_cost_usd`, so the batch's maximum spend is the per-run cap
    /// times every attempt (planned cells plus these), not times the cells alone.
    pub retried_attempts: i64,
    pub retried_cost_usd: Option<f64>,
    /// The mean over scored cells whose harness finished on its own (no `cut_short`), shown
    /// next to the batch mean when some cells were cut short; `None` when none were.
    pub mean_score_finished: Option<f64>,
    /// What was benchmarked: every distinct harness the batch's runs used (normally one),
    /// read from pb's own per-run copy. Empty when no run has a copy.
    pub harnesses: Vec<crate::harness::HarnessInfo>,
    /// What a replay batch (`pb replay`, or one model of a `pb sweep`) replayed —
    /// one entry per distinct (source run, from node, through node) among its runs; empty for
    /// an ordinary batch.
    pub replays: Vec<ReplayContext>,
}

/// One batch's task × seed matrix, by its id (`list_batches`' `id`, including the `label:`
/// pseudo-batch form). `None` when no such batch (or label) exists.
pub fn batch_detail(pb_root: &Path, id: &str) -> Result<Option<BatchDetail>> {
    let conn = open_ro(pb_root)?;
    if let Some(label_part) = id.strip_prefix("label:") {
        return pseudo_batch_detail(&conn, pb_root, label_part);
    }
    real_batch_detail(&conn, pb_root, id)
}

/// What a batch page shows that does not come from the batch's runs.
struct Header {
    id: String,
    label: Option<String>,
    version_id: Option<String>,
    limits_json: Option<String>,
    parallel: Option<i64>,
    started_at: Option<i64>,
    ended_at: Option<i64>,
    status: BatchStatus,
    tasks: Vec<String>,
    seeds: Vec<i64>,
    /// A real batch plans its whole grid: a cell pb has not started still shows.
    planned: bool,
}

fn real_batch_detail(conn: &Connection, pb_root: &Path, id: &str) -> Result<Option<BatchDetail>> {
    let Some(batch) = BatchRow::find(conn, id) else {
        return Ok(None);
    };
    let status = batch.status();
    let runs = query_runs(
        conn,
        "SELECT * FROM runs WHERE batch_id = ?1 ORDER BY started_at ASC",
        [id],
    )?;
    let starting = status == BatchStatus::Running && runs.is_empty();
    let start_log = if starting {
        start_log_tail(pb_root, id)
    } else {
        None
    };
    let header = Header {
        id: id.to_owned(),
        tasks: batch.tasks(),
        seeds: batch.seed_list(),
        label: batch.label,
        version_id: batch.version_id,
        limits_json: batch.limits_json,
        parallel: batch.parallel,
        started_at: Some(ms(batch.started_at)),
        ended_at: ms_opt(batch.ended_at),
        status,
        planned: true,
    };
    Ok(Some(BatchDetail {
        starting,
        start_log,
        ..detail_from_runs(conn, pb_root, header, runs)
    }))
}

fn pseudo_batch_detail(
    conn: &Connection,
    pb_root: &Path,
    label_part: &str,
) -> Result<Option<BatchDetail>> {
    let label = if label_part.is_empty() {
        None
    } else {
        Some(label_part)
    };
    let batch_col = has_column(conn, "runs", "batch_id");
    let Some(batch) = PseudoBatch::find(conn, batch_col, label)? else {
        return Ok(None);
    };
    let (started_at, ended_at) = (batch.started_at(), batch.ended_at());
    let header = Header {
        id: batch.id(),
        label: batch.label.clone(),
        version_id: batch.version_id(conn, batch_col),
        limits_json: batch.limits_json(),
        parallel: None,
        started_at: started_at.is_finite().then(|| ms(started_at)),
        ended_at: ended_at.is_finite().then(|| ms(ended_at)),
        status: batch.status(),
        tasks: batch.tasks(),
        seeds: batch.seeds(),
        planned: false,
    };
    Ok(Some(detail_from_runs(conn, pb_root, header, batch.runs)))
}

/// The page of a batch with these runs; `starting` and `start_log` are left for the caller.
fn detail_from_runs(
    conn: &Connection,
    pb_root: &Path,
    header: Header,
    runs: Vec<RunRow>,
) -> BatchDetail {
    let identity = harness_identity(conn, &header.version_id);
    let by_cell = group_attempts(runs);
    let (total_cost_usd, total_wall_s) = total_cost_and_wall(&by_cell);
    let retried = count_cells(&by_cell, false);
    let batch_crashed = header.status == BatchStatus::Crashed;
    let matrix = build_matrix(
        &header.tasks,
        &header.seeds,
        &by_cell,
        batch_crashed,
        header.planned.then_some(&header.status),
    );
    let harnesses = crate::harness::batch_harnesses(pb_root, &harness_runs(&by_cell));
    let replays = replay_contexts(conn, pb_root, &attempts_oldest_first(&by_cell));
    BatchDetail {
        id: header.id,
        starting: false,
        start_log: None,
        label: header.label,
        harness_version: identity.version,
        zeroshot_version: identity.zeroshot_version,
        models: identity.models,
        limits: limits(&header.limits_json),
        parallel: header.parallel,
        started_at: header.started_at,
        ended_at: header.ended_at,
        status: header.status,
        tasks: header.tasks,
        seeds: header.seeds,
        mean_score: matrix.overall.map(|s| s.mean),
        min_score: matrix.overall.map(|s| s.min),
        max_score: matrix.overall.map(|s| s.max),
        mean_score_finished: mean_finished(&matrix.cells),
        cells: matrix.cells,
        row_aggregates: matrix.rows,
        col_aggregates: matrix.cols,
        total_cost_usd,
        total_wall_s,
        retried_attempts: retried.retried,
        retried_cost_usd: retried.retried_cost,
        harnesses,
        replays,
    }
}

/// The mean over cells scored without being cut short, when any scored cell was cut short.
fn mean_finished(cells: &[Cell]) -> Option<f64> {
    if !cells.iter().any(|c| c.cut_short.is_some()) {
        return None;
    }
    let scores: Vec<f64> = cells
        .iter()
        .filter(|c| c.cut_short.is_none())
        .filter_map(|c| c.score)
        .collect();
    (!scores.is_empty()).then(|| scores.iter().sum::<f64>() / scores.len() as f64)
}

/// The runs a batch's harness description reads: every attempt, newest first, with its
/// `--model` override.
fn harness_runs(by_cell: &ByCell) -> Vec<(String, Option<String>)> {
    let mut runs = all_attempts(by_cell);
    runs.sort_by(|a, b| by_start_time(b, a));
    runs.into_iter()
        .map(|r| (r.run_id.clone(), r.model_override.clone()))
        .collect()
}

/// Every attempt of every cell, oldest first (ties by run id).
fn attempts_oldest_first(by_cell: &ByCell) -> Vec<&RunRow> {
    let mut runs = all_attempts(by_cell);
    runs.sort_by(|a, b| by_start_time(a, b).then_with(|| a.run_id.cmp(&b.run_id)));
    runs
}

/// Summed over every recorded attempt of every cell, including attempts a cell's shown run
/// replaced: a retried attempt's cost stays visible.
fn total_cost_and_wall(by_cell: &ByCell) -> (Option<f64>, Option<f64>) {
    let (mut cost_sum, mut cost_any, mut wall_sum, mut wall_any) = (0.0, false, 0.0, false);
    for run in by_cell.values().flatten() {
        if let Some(c) = run.cost_usd {
            cost_sum += c;
            cost_any = true;
        }
        if let Some(w) = run.wall_s {
            wall_sum += w;
            wall_any = true;
        }
    }
    (cost_any.then_some(cost_sum), wall_any.then_some(wall_sum))
}
