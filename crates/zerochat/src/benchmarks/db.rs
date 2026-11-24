//! Reading `pb.sqlite`: the connection, columns a database may not have, the queries that load
//! run rows, and the JSON and time columns.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, Row};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::run::{RunRow, row_to_run};
use crate::pb_db;

pub(super) fn open_ro(pb_root: &Path) -> Result<Connection> {
    let path = pb_db::path(pb_root);
    pb_db::open_read_only(&path).with_context(|| format!("cannot open {}", path.display()))
}

/// A column this database might not have (an older schema, before that migration): absent is
/// `None`, same as a `NULL` value.
pub(super) fn get_opt<T: rusqlite::types::FromSql>(row: &Row, name: &str) -> Option<T> {
    row.get::<&str, Option<T>>(name).ok().flatten()
}

/// Every run a `SELECT * FROM runs ...` query returns.
pub(super) fn query_runs(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<RunRow>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row_to_run(row));
    }
    Ok(out)
}

/// The first run a `SELECT * FROM runs ...` query returns; `None` also when the query fails (a
/// column this database does not have).
pub(super) fn query_run(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Option<RunRow> {
    conn.query_row(sql, params, |r| Ok(row_to_run(r))).ok()
}

/// The `WHERE` condition for the runs label pseudo-batches group: runs with no `batch_id`, or
/// every run on a database whose `runs` has no `batch_id` column (`batch_col`).
pub(super) fn unbatched(batch_col: bool) -> &'static str {
    if batch_col { "batch_id IS NULL" } else { "1" }
}

/// The length of a JSON list column (`batches.task_ids`, `batches.seeds`); 0 when it is not one.
pub(super) fn json_len(text: &Option<String>) -> usize {
    text.as_deref()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(t).ok())
        .map(|v| v.len())
        .unwrap_or(0)
}

/// pb's times are Unix seconds; the API's are Unix milliseconds.
pub(super) fn ms(secs: f64) -> i64 {
    (secs * 1000.0).round() as i64
}

pub(super) fn ms_opt(secs: Option<f64>) -> Option<i64> {
    secs.map(ms)
}

/// A run's or a batch's caps (`limits_json`).
#[derive(Serialize, Clone, Default, TS)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_cost_usd: Option<f64>,
    pub max_wall_minutes: Option<f64>,
    pub max_steps: Option<i64>,
}

/// `None` when there is no `limits_json`, or it is not JSON.
pub(super) fn limits(limits_json: &Option<String>) -> Option<Limits> {
    let v: Value = serde_json::from_str(limits_json.as_deref()?).ok()?;
    Some(Limits {
        max_cost_usd: v.get("max_cost_usd").and_then(Value::as_f64),
        max_wall_minutes: v.get("max_wall_minutes").and_then(Value::as_f64),
        max_steps: v.get("max_steps").and_then(Value::as_i64),
    })
}

pub(super) fn cap_usd(limits_json: &Option<String>) -> Option<f64> {
    limits(limits_json).and_then(|l| l.max_cost_usd)
}
