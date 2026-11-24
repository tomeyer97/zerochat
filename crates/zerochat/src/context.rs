//! Run context: which benchmark batch or feature slice a run belongs to, and how it scored.
//! One provider per tool (pb, zsf); the first that recognises the run id wins.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;
use ts_rs::TS;

use crate::pb_db;

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
// Every `Option` field is left out of the JSON when `None`, and optional in TypeScript.
#[ts(optional_fields)]
pub struct RunContext {
    /// `"benchmark"` (pb) or `"slice"` (zsf).
    #[ts(type = "'benchmark' | 'slice'")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// zsf: the feature, slice and attempt the run belongs to (the feature page link).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    /// One short line for the run header and sidebar, already formatted, e.g.
    /// "gron · seed 1 · score 0.85" or "gron · seed 1 · evaluating".
    pub line: String,
}

/// One place a run's context can come from. `lookup` opens its own store read-only and
/// returns `None` when it has nothing for this run (not an error: most providers recognise
/// only a few runs or none).
pub trait RunContextProvider: Send + Sync {
    fn lookup(&self, run_id: &str) -> Option<RunContext>;
}

/// pb (`<pb_root>/pb.sqlite`): `runs.zeroshot_run_id` is the Zeroshot run's own id.
/// `batches` groups runs when the database has it; older pb databases (see `pb_db` for the
/// schema versions) or runs made before a batch was recorded have none, so zerochat groups by
/// `runs.label` instead.
pub struct PbProvider {
    pub sqlite_path: PathBuf,
}

struct Row {
    label: Option<String>,
    task_id: String,
    seed: i64,
    score: Option<f64>,
    status: String,
    cost_usd: Option<f64>,
    batch_id: Option<String>,
    batch_label: Option<String>,
}

impl RunContextProvider for PbProvider {
    fn lookup(&self, run_id: &str) -> Option<RunContext> {
        let connection = pb_db::open_read_only(&self.sqlite_path).ok()?;
        let row = query_with_batches(&connection, run_id)
            .or_else(|| query_without_batches(&connection, run_id))?;
        Some(to_context(row))
    }
}

/// `None` covers both "no such run" and "no `batches` table" (an older pb database): either
/// way the caller falls back to the batch-less query.
fn query_with_batches(connection: &Connection, run_id: &str) -> Option<Row> {
    connection
        .query_row(
            "SELECT r.label, r.task_id, r.seed, r.score, r.status, r.cost_usd, b.id, b.label \
             FROM runs r LEFT JOIN batches b ON b.id = r.batch_id WHERE r.zeroshot_run_id = ?1",
            [run_id],
            row_to_fields,
        )
        .ok()
}

/// The same row for a database without a `batches` table: no batch id or label.
fn query_without_batches(connection: &Connection, run_id: &str) -> Option<Row> {
    connection
        .query_row(
            "SELECT label, task_id, seed, score, status, cost_usd, NULL, NULL FROM runs WHERE zeroshot_run_id = ?1",
            [run_id],
            row_to_fields,
        )
        .ok()
}

fn row_to_fields(row: &rusqlite::Row) -> rusqlite::Result<Row> {
    Ok(Row {
        label: row.get(0)?,
        task_id: row.get(1)?,
        seed: row.get(2)?,
        score: row.get(3)?,
        status: row.get(4)?,
        cost_usd: row.get(5)?,
        batch_id: row.get(6)?,
        batch_label: row.get(7)?,
    })
}

/// What an unscored run's line says, by pb's `runs.status`. Only a harness that finished
/// cleanly (`success`) is still waiting for pb's evaluation; a run pb interrupted, or one that
/// crashed or hit a limit, never will be. A status not listed here is shown as it is.
const UNSCORED_STATES: &[(&str, &str)] = &[
    ("running", "running"),
    ("success", "evaluating"),
    ("interrupted", "interrupted"),
    ("crash", "crashed"),
    ("crashed", "crashed"),
    ("timeout", "over time limit"),
    ("budget_exceeded", "over budget"),
    ("step_limit", "over step limit"),
];

fn run_state(score: Option<f64>, status: &str) -> String {
    if let Some(score) = score {
        return format!("score {score:.2}");
    }
    let label = UNSCORED_STATES
        .iter()
        .find(|(pb_status, _)| *pb_status == status)
        .map_or(status, |(_, label)| *label);
    label.to_owned()
}

fn to_context(row: Row) -> RunContext {
    let state = run_state(row.score, &row.status);
    RunContext {
        kind: "benchmark",
        parent: row.batch_id,
        label: row.batch_label.or(row.label),
        task: Some(row.task_id.clone()),
        seed: Some(row.seed),
        score: row.score,
        cost_usd: row.cost_usd,
        feature: None,
        slice: None,
        attempt: None,
        line: format!("{} · seed {} · {state}", row.task_id, row.seed),
    }
}

/// zsf (`<zsf state root>/<feature>/state.json`): every attempt's run (and the runs it resumed
/// from) names its slice. The index is built on the first lookup, once per provider.
pub struct ZsfProvider {
    pub root: PathBuf,
    index: std::sync::OnceLock<crate::features::SliceIndex>,
}

impl ZsfProvider {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            index: std::sync::OnceLock::new(),
        }
    }
}

impl RunContextProvider for ZsfProvider {
    fn lookup(&self, run_id: &str) -> Option<RunContext> {
        let run = self
            .index
            .get_or_init(|| crate::features::SliceIndex::build(&self.root))
            .runs
            .get(run_id)?;
        Some(RunContext {
            kind: "slice",
            parent: Some(run.feature.clone()),
            label: Some(run.title.clone()),
            // The sidebar's label: title, attempt and what became of the attempt's work.
            task: Some(crate::features::context_label(run)),
            seed: None,
            score: None,
            cost_usd: None,
            feature: Some(run.feature.clone()),
            slice: Some(run.slice.clone()),
            attempt: Some(run.attempt),
            line: crate::features::context_line(run),
        })
    }
}

/// Every configured provider. Looked up in order; the first match wins.
pub fn providers(
    pb_root: Option<&Path>,
    zsf_root: Option<&Path>,
) -> Vec<Box<dyn RunContextProvider>> {
    let mut out: Vec<Box<dyn RunContextProvider>> = Vec::new();
    if let Some(root) = pb_root {
        out.push(Box::new(PbProvider {
            sqlite_path: pb_db::path(root),
        }));
    }
    if let Some(root) = zsf_root.filter(|r| r.is_dir()) {
        out.push(Box::new(ZsfProvider::new(root.to_path_buf())));
    }
    out
}

pub fn lookup(providers: &[Box<dyn RunContextProvider>], run_id: &str) -> Option<RunContext> {
    providers.iter().find_map(|p| p.lookup(run_id))
}

/// The types of this module the web reads (`web/src/generated/context.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, context: RunContext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_db(path: &Path, with_batches: bool) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE runs (run_id TEXT PRIMARY KEY, label TEXT, task_id TEXT, seed INTEGER, score REAL, \
                 status TEXT, cost_usd REAL, zeroshot_run_id TEXT, batch_id TEXT);",
            )
            .unwrap();
        if with_batches {
            connection
                .execute_batch("CREATE TABLE batches (id TEXT PRIMARY KEY, label TEXT);")
                .unwrap();
            connection
                .execute(
                    "INSERT INTO batches (id, label) VALUES ('b1','try-planner')",
                    [],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO runs (run_id, label, task_id, seed, score, status, cost_usd, zeroshot_run_id, batch_id) \
                 VALUES ('r1','baseline','gron',1,0.85,'success',0.42,'zs-1', ?1)",
                [if with_batches { Some("b1") } else { None }],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO runs (run_id, label, task_id, seed, score, status, cost_usd, zeroshot_run_id, batch_id) \
                 VALUES ('r2','baseline','gron',2,NULL,'running',0.10,'zs-2', NULL)",
                [],
            )
            .unwrap();
    }

    #[test]
    fn scored_run_with_batches_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pb.sqlite");
        seed_db(&path, true);
        let provider = PbProvider { sqlite_path: path };
        let context = provider.lookup("zs-1").unwrap();
        assert_eq!(context.kind, "benchmark");
        assert_eq!(context.parent, Some("b1".to_owned()));
        assert_eq!(context.label, Some("try-planner".to_owned()));
        assert_eq!(context.line, "gron · seed 1 · score 0.85");
    }

    #[test]
    fn unscored_running_run_says_running_not_evaluating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pb.sqlite");
        seed_db(&path, true);
        let provider = PbProvider { sqlite_path: path };
        let context = provider.lookup("zs-2").unwrap();
        assert_eq!(context.line, "gron · seed 2 · running");
        assert!(context.parent.is_none());
    }

    #[test]
    fn an_unscored_run_pb_ended_says_how_not_evaluating() {
        let row = |status: &str| Row {
            label: None,
            task_id: "gron".into(),
            seed: 0,
            score: None,
            status: status.into(),
            cost_usd: None,
            batch_id: None,
            batch_label: None,
        };
        assert_eq!(to_context(row("crash")).line, "gron · seed 0 · crashed");
        assert_eq!(
            to_context(row("interrupted")).line,
            "gron · seed 0 · interrupted"
        );
        assert_eq!(
            to_context(row("success")).line,
            "gron · seed 0 · evaluating"
        );
        assert_eq!(to_context(row("crashed")).line, "gron · seed 0 · crashed");
        assert_eq!(
            to_context(row("timeout")).line,
            "gron · seed 0 · over time limit"
        );
        assert_eq!(
            to_context(row("budget_exceeded")).line,
            "gron · seed 0 · over budget"
        );
        assert_eq!(
            to_context(row("step_limit")).line,
            "gron · seed 0 · over step limit"
        );
        assert_eq!(
            to_context(row("queued")).line,
            "gron · seed 0 · queued",
            "an unknown status is shown as it is"
        );
    }

    #[test]
    fn older_database_without_batches_table_falls_back_to_label() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pb.sqlite");
        seed_db(&path, false);
        let provider = PbProvider { sqlite_path: path };
        let context = provider.lookup("zs-1").unwrap();
        assert_eq!(context.parent, None);
        assert_eq!(context.label, Some("baseline".to_owned()));
        assert_eq!(context.line, "gron · seed 1 · score 0.85");
    }

    #[test]
    fn unknown_run_is_none_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pb.sqlite");
        seed_db(&path, true);
        let provider = PbProvider { sqlite_path: path };
        assert!(provider.lookup("not-a-run").is_none());
    }

    #[test]
    fn missing_database_is_none_not_a_panic() {
        let provider = PbProvider {
            sqlite_path: PathBuf::from("/does/not/exist/pb.sqlite"),
        };
        assert!(provider.lookup("zs-1").is_none());
    }
}
