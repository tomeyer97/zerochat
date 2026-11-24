//! One run's score card: its cell state, failing tests, step scores and cost per node.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use ts_rs::TS;

use super::db::{cap_usd, open_ro, query_run};
use super::run::cell_state;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StepScore {
    pub seq: i64,
    pub node: String,
    pub score: Option<f64>,
    pub tests_passed: Option<i64>,
    pub tests_total: Option<i64>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NodeCost {
    pub node: String,
    pub cost_usd: f64,
    pub calls: i64,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunScore {
    pub run_id: String,
    pub zeroshot_run_id: Option<String>,
    pub task: String,
    pub seed: i64,
    pub label: Option<String>,
    pub batch_id: Option<String>,
    #[ts(type = "BenchmarkCellState")]
    pub state: String,
    pub exit_reason: Option<String>,
    pub over_limit: Option<String>,
    pub score: Option<f64>,
    pub tests_passed: Option<i64>,
    pub tests_total: Option<i64>,
    pub failing_tests: Vec<String>,
    pub regression_passed: Option<i64>,
    pub regression_total: Option<i64>,
    pub cost_usd: Option<f64>,
    pub reported_cost_usd: Option<f64>,
    pub cap_usd: Option<f64>,
    pub wall_s: Option<f64>,
    pub steps: Vec<StepScore>,
    pub node_costs: Vec<NodeCost>,
}

/// A run's score card, found by the Zeroshot run id it ran as (`zeroshot_run_id`: a zerochat run
/// page only has that one) or else by pb's own `run_id`. `None` when pb has no such run.
pub fn run_score(pb_root: &Path, run: &str) -> Result<Option<RunScore>> {
    let conn = open_ro(pb_root)?;
    // Two queries, not one `?1 IN (...)`: the Zeroshot id wins, and a database older than the
    // `zeroshot_run_id` column fails the first query and is still searched by `run_id`.
    let found = query_run(
        &conn,
        "SELECT * FROM runs WHERE zeroshot_run_id = ?1",
        [run],
    )
    .or_else(|| query_run(&conn, "SELECT * FROM runs WHERE run_id = ?1", [run]));
    let Some(run_row) = found else {
        return Ok(None);
    };
    // A lone run page has no enclosing batch to reconcile against: a run still
    // `status='running'` reads as live.
    let (state, exit_reason, over_limit) = cell_state(&run_row, false);

    Ok(Some(RunScore {
        failing_tests: failing_tests(&conn, &run_row.run_id)?,
        steps: step_scores(&conn, &run_row.run_id)?,
        node_costs: node_costs(&conn, &run_row.run_id)?,
        // The run row's own columns, under the API's names.
        run_id: run_row.run_id.clone(),
        zeroshot_run_id: run_row.zeroshot_run_id.clone(),
        task: run_row.task_id.clone(),
        seed: run_row.seed,
        label: run_row.label.clone(),
        batch_id: run_row.batch_id.clone(),
        state: state.to_owned(),
        exit_reason,
        over_limit: over_limit.map(str::to_owned),
        score: run_row.score,
        tests_passed: run_row.tests_passed,
        tests_total: run_row.tests_total,
        regression_passed: run_row.regression_passed,
        regression_total: run_row.regression_total,
        cost_usd: run_row.cost_usd,
        reported_cost_usd: run_row.reported_cost_usd,
        cap_usd: cap_usd(&run_row.limits_json),
        wall_s: run_row.wall_s,
    }))
}

/// The run's tests that did not pass, as `branch/name`.
fn failing_tests(conn: &Connection, run_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT branch, name FROM test_results \
         WHERE run_id = ?1 AND status != 'passed' ORDER BY branch, name",
    )?;
    let rows = stmt.query_map([run_id], |r| {
        let branch: String = r.get(0)?;
        let name: String = r.get(1)?;
        Ok(format!("{branch}/{name}"))
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// The score after each scored step, in order.
fn step_scores(conn: &Connection, run_id: &str) -> Result<Vec<StepScore>> {
    let mut stmt = conn.prepare(
        "SELECT seq, node_id, score, tests_passed, tests_total FROM step_scores \
         WHERE run_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map([run_id], |r| {
        Ok(StepScore {
            seq: r.get(0)?,
            node: r.get(1)?,
            score: r.get(2)?,
            tests_passed: r.get(3)?,
            tests_total: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Cost and number of executions per node, the most expensive first.
fn node_costs(conn: &Connection, run_id: &str) -> Result<Vec<NodeCost>> {
    let mut stmt = conn.prepare(
        "SELECT node_name, SUM(cost_usd), COUNT(*) FROM node_executions \
         WHERE run_id = ?1 GROUP BY node_name ORDER BY SUM(cost_usd) DESC",
    )?;
    let rows = stmt.query_map([run_id], |r| {
        Ok(NodeCost {
            node: r.get(0)?,
            cost_usd: r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
            calls: r.get(2)?,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}
