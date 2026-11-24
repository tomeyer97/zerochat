//! Two batches compared per task (score change against seed-to-seed noise) and per node (mean
//! cost and time), as pb's own `compare` does.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, params_from_iter};
use serde::Serialize;
use ts_rs::TS;

use super::db::{open_ro, query_runs, unbatched};
use super::lists::sorted_union_keys;
use super::matrix::Stats;
use super::run::RunRow;
use crate::pb_db::{has_column, has_table};

/// One group's (a batch's own runs) stats for one task: the exact shape pb's `stats.per_task`
/// computes, restricted to this batch's run rows, `status != 'running'` and `score IS NOT NULL`
/// (unscored runs are excluded entirely, same as pb's CLI `compare`, not averaged in as zero).
#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskAgg {
    pub n: i64,
    pub mean_score: f64,
    pub min_score: f64,
    pub max_score: f64,
    pub mean_cost_usd: Option<f64>,
    pub mean_time_s: Option<f64>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskCompareRow {
    pub task: String,
    pub a: Option<TaskAgg>,
    pub b: Option<TaskAgg>,
    pub delta_score: Option<f64>,
    pub delta_cost_usd: Option<f64>,
    pub delta_time_s: Option<f64>,
    pub noise: Noise,
}

/// Whether a task's score change between two batches is more than seed-to-seed noise (pb's own
/// verdict, ported exactly).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
// The variant names are the serialized API values (`within_noise`, ...); renaming one changes the API.
#[allow(clippy::enum_variant_names)]
pub enum Noise {
    /// The change is no larger than the wider min–max spread of the two sides.
    WithinNoise,
    BeyondSpread,
    /// A side has fewer than `MIN_SEEDS_FOR_SPREAD` scored runs: no spread to compare with.
    NeedsSeeds,
    /// One side has no scored runs.
    #[serde(rename = "n_a")]
    NotApplicable,
}

/// The fewest scored runs per side that give a score spread.
const MIN_SEEDS_FOR_SPREAD: i64 = 2;

#[derive(Serialize, Clone, TS)]
#[serde(rename_all = "camelCase")]
pub struct NodeAgg {
    pub execs: i64,
    pub mean_cost_usd: Option<f64>,
    pub mean_time_s: Option<f64>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NodeCompareRow {
    pub node: String,
    pub a: Option<NodeAgg>,
    pub b: Option<NodeAgg>,
    pub delta_mean_cost_usd: Option<f64>,
    pub delta_mean_time_s: Option<f64>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TotalsRow {
    pub tasks: usize,
    pub delta_score: f64,
    pub delta_cost_usd: f64,
    pub delta_time_s: f64,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BatchCompare {
    pub a_id: String,
    pub a_label: Option<String>,
    pub a_run_count: usize,
    pub b_id: String,
    pub b_label: Option<String>,
    pub b_run_count: usize,
    pub tasks: Vec<TaskCompareRow>,
    /// Mean of the per-task deltas over tasks where both sides have scored runs (pb CLI's "mean
    /// over tasks" row). `None` when no task has both.
    pub totals: Option<TotalsRow>,
    pub nodes: Vec<NodeCompareRow>,
}

/// A batch's own label and run rows, by its id (`list_batches`' `id`, including the `label:`
/// pseudo-batch form). Unlike `batch_detail`, a batch nobody has heard of is not an error: it is
/// simply zero runs, so `compare_batches` never needs to special-case "not found" — both sides
/// of a typo'd comparison just show up empty.
fn fetch_batch_runs(conn: &Connection, id: &str) -> Result<(Option<String>, Vec<RunRow>)> {
    if let Some(label_part) = id.strip_prefix("label:") {
        let label = if label_part.is_empty() {
            None
        } else {
            Some(label_part)
        };
        let sql = format!(
            "SELECT * FROM runs WHERE {} AND label IS ?1",
            unbatched(has_column(conn, "runs", "batch_id"))
        );
        return Ok((label.map(str::to_owned), query_runs(conn, &sql, [label])?));
    }
    if !has_table(conn, "batches") {
        return Ok((None, Vec::new()));
    }
    let label = conn
        .query_row("SELECT label FROM batches WHERE id=?1", [id], |r| {
            r.get::<_, Option<String>>(0)
        })
        .ok()
        .flatten();
    Ok((
        label,
        query_runs(conn, "SELECT * FROM runs WHERE batch_id = ?1", [id])?,
    ))
}

/// pb's `stats.per_task`, scoped to one batch's own runs instead of a label/version group:
/// group scored (`score IS NOT NULL`), non-running rows by task, with the mean/min/max score and
/// the mean cost/time over that SAME filtered set (pb's single `WHERE ... GROUP BY` does both
/// together, not two independent filters).
fn per_task_agg(runs: &[RunRow]) -> HashMap<String, TaskAgg> {
    let mut groups: HashMap<String, Vec<&RunRow>> = HashMap::new();
    for r in runs {
        if r.status == "running" || r.score.is_none() {
            continue;
        }
        groups.entry(r.task_id.clone()).or_default().push(r);
    }
    groups
        .into_iter()
        .filter_map(|(task, rs)| {
            let scores: Vec<f64> = rs.iter().filter_map(|r| r.score).collect();
            let costs: Vec<f64> = rs.iter().filter_map(|r| r.cost_usd).collect();
            let times: Vec<f64> = rs.iter().filter_map(|r| r.wall_s).collect();
            // Every run in a group has a score, so the group's stats always exist.
            let stats = Stats::of(&scores)?;
            let agg = TaskAgg {
                n: scores.len() as i64,
                mean_score: stats.mean,
                min_score: stats.min,
                max_score: stats.max,
                mean_cost_usd: mean(&costs),
                mean_time_s: mean(&times),
            };
            Some((task, agg))
        })
        .collect()
}

/// pb's `stats.node_stats` (mean cost/time per node name) over exactly these run ids.
fn node_cost_time_agg<'a>(
    conn: &Connection,
    run_ids: impl Iterator<Item = &'a str>,
) -> Result<HashMap<String, NodeAgg>> {
    let ids: Vec<&str> = run_ids.collect();
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let duration = if has_column(conn, "node_executions", "duration_s") {
        "AVG(duration_s)"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT node_name, COUNT(*), {duration}, AVG(cost_usd) FROM node_executions WHERE run_id IN ({placeholders}) GROUP BY node_name"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(ids.iter()))?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        out.insert(
            name,
            NodeAgg {
                execs: row.get(1)?,
                mean_time_s: row.get(2)?,
                mean_cost_usd: row.get(3)?,
            },
        );
    }
    Ok(out)
}

/// The runs the per-node table averages over, matched the way the task table is: scored,
/// finished runs (`status != 'running'`, `score IS NOT NULL`) of `shared_tasks` (tasks both sides
/// scored), so a task only one side ran does not show up as a per-node cost difference. With no
/// shared task there is nothing to compare, and the per-node table has no rows.
fn matched_scored_run_ids<'a>(
    runs: &'a [RunRow],
    shared_tasks: &HashSet<String>,
) -> impl Iterator<Item = &'a str> {
    runs.iter()
        .filter(|r| r.status != "running" && r.score.is_some())
        .filter(move |r| shared_tasks.contains(&r.task_id))
        .map(|r| r.run_id.as_str())
}

fn compare_nodes(
    conn: &Connection,
    a_runs: &[RunRow],
    b_runs: &[RunRow],
    shared_tasks: &HashSet<String>,
) -> Result<Vec<NodeCompareRow>> {
    let a = node_cost_time_agg(conn, matched_scored_run_ids(a_runs, shared_tasks))?;
    let b = node_cost_time_agg(conn, matched_scored_run_ids(b_runs, shared_tasks))?;
    Ok(sorted_union_keys(&a, &b)
        .into_iter()
        .map(|node| {
            let av = a.get(&node).cloned();
            let bv = b.get(&node).cloned();
            let mean_cost = |agg: &Option<NodeAgg>| agg.as_ref().and_then(|x| x.mean_cost_usd);
            let mean_time = |agg: &Option<NodeAgg>| agg.as_ref().and_then(|x| x.mean_time_s);
            let delta_mean_cost_usd = delta(mean_cost(&av), mean_cost(&bv));
            let delta_mean_time_s = delta(mean_time(&av), mean_time(&bv));
            NodeCompareRow {
                node,
                a: av,
                b: bv,
                delta_mean_cost_usd,
                delta_mean_time_s,
            }
        })
        .collect())
}

/// B's value minus A's, when both sides have one.
fn delta(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    Some(b? - a?)
}

/// The mean, `None` for no values.
fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// Two batches, per task (score Δ, min–max spread per side, pb's noise verdict ported exactly:
/// `|Δscore| ≤ the larger of the two sides' min–max spreads`, and "needs 2 seeds" instead of
/// averaging in a single-seed task as if its spread were zero) plus per node (mean cost/time Δ,
/// like `pb nodes`). `a`/`b` are batch ids as `list_batches` prints them, including the `label:`
/// pseudo-batch form; either side may have zero runs (shown as empty, not an error — see
/// `fetch_batch_runs`).
pub fn compare_batches(pb_root: &Path, a_id: &str, b_id: &str) -> Result<BatchCompare> {
    let conn = open_ro(pb_root)?;
    let (a_label, a_runs) = fetch_batch_runs(&conn, a_id)?;
    let (b_label, b_runs) = fetch_batch_runs(&conn, b_id)?;
    let a_agg = per_task_agg(&a_runs);
    let b_agg = per_task_agg(&b_runs);

    let rows: Vec<TaskCompareRow> = sorted_union_keys(&a_agg, &b_agg)
        .into_iter()
        .map(|task| {
            let a = a_agg.get(&task).cloned();
            let b = b_agg.get(&task).cloned();
            let (delta_score, delta_cost_usd, delta_time_s, noise) = match (&a, &b) {
                (Some(a), Some(b)) => {
                    let delta_score = b.mean_score - a.mean_score;
                    let delta_cost_usd =
                        b.mean_cost_usd.unwrap_or(0.0) - a.mean_cost_usd.unwrap_or(0.0);
                    let delta_time_s = b.mean_time_s.unwrap_or(0.0) - a.mean_time_s.unwrap_or(0.0);
                    let noise = if a.n < MIN_SEEDS_FOR_SPREAD || b.n < MIN_SEEDS_FOR_SPREAD {
                        Noise::NeedsSeeds
                    } else {
                        let spread = (a.max_score - a.min_score).max(b.max_score - b.min_score);
                        if delta_score.abs() <= spread {
                            Noise::WithinNoise
                        } else {
                            Noise::BeyondSpread
                        }
                    };
                    (
                        Some(delta_score),
                        Some(delta_cost_usd),
                        Some(delta_time_s),
                        noise,
                    )
                }
                _ => (None, None, None, Noise::NotApplicable),
            };
            TaskCompareRow {
                task,
                a,
                b,
                delta_score,
                delta_cost_usd,
                delta_time_s,
                noise,
            }
        })
        .collect();

    let both: Vec<&TaskCompareRow> = rows
        .iter()
        .filter(|r| r.a.is_some() && r.b.is_some())
        .collect();
    let shared_tasks: HashSet<String> = both.iter().map(|r| r.task.clone()).collect();
    let totals = (!both.is_empty()).then(|| {
        let n = both.len() as f64;
        TotalsRow {
            tasks: both.len(),
            delta_score: both.iter().filter_map(|r| r.delta_score).sum::<f64>() / n,
            delta_cost_usd: both.iter().filter_map(|r| r.delta_cost_usd).sum::<f64>() / n,
            delta_time_s: both.iter().filter_map(|r| r.delta_time_s).sum::<f64>() / n,
        }
    });

    let nodes = compare_nodes(&conn, &a_runs, &b_runs, &shared_tasks)?;
    Ok(BatchCompare {
        a_id: a_id.to_owned(),
        a_label,
        a_run_count: a_runs.len(),
        b_id: b_id.to_owned(),
        b_label,
        b_run_count: b_runs.len(),
        tasks: rows,
        totals,
        nodes,
    })
}
