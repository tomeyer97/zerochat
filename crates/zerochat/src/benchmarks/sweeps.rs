//! The sweeps list (`pb sweeps`).

use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::db::{ms, open_ro};
use super::lists::push_unique;
use crate::pb_db::has_table;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SweepSummary {
    pub sweep_id: String,
    pub created_at: i64,
    pub source_run: String,
    pub label: Option<String>,
    pub task_id: Option<String>,
    pub items: Vec<String>,
    pub models: Vec<String>,
    pub n: i64,
    pub runs: i64,
    pub running: i64,
    pub cost_usd: Option<f64>,
}

/// Each sweep with its source run's task, and how many runs it has, how many are still running
/// and what they cost.
const SWEEPS_SQL: &str = r#"
    SELECT s.sweep_id, s.created_at, s.source_run, s.label, s.spec_json, src.task_id,
           COUNT(r.run_id), SUM(r.status = 'running'), SUM(r.cost_usd)
    FROM sweeps s
    LEFT JOIN runs r ON r.sweep_id = s.sweep_id
    LEFT JOIN runs src ON src.run_id = s.source_run
    GROUP BY s.sweep_id
    ORDER BY s.created_at DESC
"#;

/// `pb sweeps`: one row per sweep, newest first. Empty (not an error) on a database with no
/// `sweeps` table yet.
pub fn list_sweeps(pb_root: &Path) -> Result<Vec<SweepSummary>> {
    let conn = open_ro(pb_root)?;
    if !has_table(&conn, "sweeps") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(SWEEPS_SQL)?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let created_at: f64 = row.get(1)?;
        let spec_json: String = row.get(4)?;
        let spec: Value = serde_json::from_str(&spec_json).unwrap_or(Value::Null);
        let spec_items: Vec<&Value> = spec["items"].as_array().into_iter().flatten().collect();
        let items: Vec<String> = spec_items
            .iter()
            .filter_map(|i| i["item"].as_str().map(str::to_owned))
            .collect();
        let mut models: Vec<String> = Vec::new();
        let item_models = spec_items
            .iter()
            .flat_map(|i| i["models"].as_array().into_iter().flatten());
        for model in item_models.filter_map(Value::as_str) {
            push_unique(&mut models, model);
        }
        out.push(SweepSummary {
            sweep_id: row.get(0)?,
            created_at: ms(created_at),
            source_run: row.get(2)?,
            label: row.get(3)?,
            task_id: row.get(5)?,
            items,
            models,
            n: spec["n"].as_i64().unwrap_or(0),
            runs: row.get(6)?,
            running: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
            cost_usd: row.get(8)?,
        });
    }
    Ok(out)
}
