//! What a replay batch (`pb replay`, or one model of a `pb sweep`) replayed: the recorded run
//! and segment, the models the replays used, and the recorded baseline to read their scores
//! against.

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::db::query_run;
use super::run::RunRow;
use crate::paths;
use crate::validate::is_plain_name;

/// A group of replays of one recorded run's segment, with the recorded baseline to read their
/// scores against (the same baseline `pb replays` prints: the source run's step score after the
/// segment's last node, and that node's recorded model).
#[derive(Serialize, Clone, Debug, Default, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReplayContext {
    /// pb's run id of the recorded run, and its Zeroshot run id (the run page link).
    pub source_run: String,
    pub source_zeroshot_run_id: Option<String>,
    /// The batch page the source run sits on (`batch_id`, else `label:<label>`).
    pub source_batch: Option<String>,
    pub source_label: Option<String>,
    pub source_task: Option<String>,
    /// The source's final score (after its whole graph).
    pub source_score: Option<f64>,
    /// pb node ids (`build#1`): the first node replayed and, for a segment, the last.
    pub node: String,
    pub through: Option<String>,
    /// What the replay started from (`replay.json`'s `base_label`: "pristine task workspace",
    /// "build#1 (snapshot)").
    pub base: Option<String>,
    /// The model each replay ran the first replayed node on, distinct, in run order.
    pub models: Vec<String>,
    /// The recorded run's model for that node, and its score right after the segment.
    pub recorded_model: Option<String>,
    pub recorded_score: Option<f64>,
    pub sweep_id: Option<String>,
    pub sweep_label: Option<String>,
    /// How many of the batch's runs are replays of this segment.
    pub runs: usize,
}

/// The segment a run replays: the recorded run, its first replayed node and (for a segment of
/// several nodes) its last.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ReplayKey {
    source: String,
    node: String,
    through: Option<String>,
}

/// The recorded run's side of a segment.
struct RecordedBaseline {
    /// The node name of the first replayed node (`build` for `build#1`).
    node_name: String,
    /// The model it ran that node on.
    model: Option<String>,
    /// Its step score right after the segment's last node.
    score: Option<f64>,
}

/// The replay context of a batch's runs (see `ReplayContext`), one per segment, in the order of
/// each segment's first run. Every lookup is best-effort: a database without
/// `node_executions` / `step_scores` / `sweeps` (or without that run) leaves the field empty.
pub(super) fn replay_contexts(
    conn: &Connection,
    pb_root: &Path,
    runs: &[&RunRow],
) -> Vec<ReplayContext> {
    let mut groups: Vec<(ReplayKey, Vec<&RunRow>)> = Vec::new();
    for &run in runs {
        let Some(key) = replay_origin(pb_root, run) else {
            continue;
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(run),
            None => groups.push((key, vec![run])),
        }
    }
    groups
        .into_iter()
        .map(|(key, members)| replay_context(conn, pb_root, key, &members))
        .collect()
}

fn replay_context(
    conn: &Connection,
    pb_root: &Path,
    key: ReplayKey,
    members: &[&RunRow],
) -> ReplayContext {
    let src = query_run(conn, "SELECT * FROM runs WHERE run_id = ?1", [&key.source]);
    let recorded = recorded_baseline(conn, &key);
    let sweep_id = members.iter().find_map(|r| r.sweep_id.clone());
    let sweep_label = sweep_id.as_ref().and_then(|s| {
        conn.query_row("SELECT label FROM sweeps WHERE sweep_id = ?1", [s], |r| {
            r.get::<_, Option<String>>(0)
        })
        .ok()
        .flatten()
    });
    let base = members
        .iter()
        .find_map(|r| replay_json(pb_root, &r.run_id))
        .and_then(|v| v["base_label"].as_str().map(str::to_owned));
    ReplayContext {
        source_zeroshot_run_id: src.as_ref().and_then(|s| s.zeroshot_run_id.clone()),
        source_batch: src.as_ref().map(|s| {
            s.batch_id
                .clone()
                .unwrap_or_else(|| format!("label:{}", s.label.clone().unwrap_or_default()))
        }),
        source_label: src.as_ref().and_then(|s| s.label.clone()),
        source_task: src.as_ref().map(|s| s.task_id.clone()),
        source_score: src.as_ref().and_then(|s| s.score),
        source_run: key.source,
        node: key.node,
        through: key.through,
        base,
        models: replay_models(conn, pb_root, members, &recorded.node_name),
        recorded_model: recorded.model,
        recorded_score: recorded.score,
        sweep_id,
        sweep_label,
        runs: members.len(),
    }
}

/// The segment a run replays: the replay columns of `runs`, else `harness/replay.json` (a
/// database from before pb kept the columns). `None` for a run that is not a replay.
fn replay_origin(pb_root: &Path, run: &RunRow) -> Option<ReplayKey> {
    if let (Some(source), Some(node)) = (&run.replay_of, &run.replay_node) {
        return Some(ReplayKey {
            source: source.clone(),
            node: node.clone(),
            through: run.replay_through.clone(),
        });
    }
    let file = replay_json(pb_root, &run.run_id)?;
    let (source, node) = (file["replay_of"].as_str()?, file["node_id"].as_str()?);
    Some(ReplayKey {
        source: source.to_owned(),
        node: node.to_owned(),
        through: file["through"].as_str().map(str::to_owned),
    })
}

const NODE_EXECUTION_SQL: &str =
    "SELECT node_name, model, execution FROM node_executions WHERE run_id = ?1 AND node_id = ?2";
const EXECUTION_SQL: &str =
    "SELECT execution FROM node_executions WHERE run_id = ?1 AND node_id = ?2";
const STEP_SCORE_SQL: &str = "SELECT score FROM step_scores WHERE run_id = ?1 AND execution = ?2";

fn recorded_baseline(conn: &Connection, key: &ReplayKey) -> RecordedBaseline {
    let first: Option<(String, Option<String>, Option<i64>)> = conn
        .query_row(NODE_EXECUTION_SQL, [&key.source, &key.node], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .ok();
    let node_name = first
        .as_ref()
        .map(|f| f.0.clone())
        .unwrap_or_else(|| node_name_of(&key.node).to_owned());
    let last_execution = match &key.through {
        Some(through) => conn
            .query_row(EXECUTION_SQL, [&key.source, through], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .ok()
            .flatten(),
        None => first.as_ref().and_then(|f| f.2),
    };
    let score = last_execution.and_then(|e| {
        conn.query_row(STEP_SCORE_SQL, rusqlite::params![key.source, e], |r| {
            r.get::<_, Option<f64>>(0)
        })
        .ok()
        .flatten()
    });
    RecordedBaseline {
        node_name,
        model: first.and_then(|f| f.1),
        score,
    }
}

const REPLAY_MODEL_SQL: &str = "SELECT model FROM node_executions WHERE run_id = ?1 AND node_name = ?2 ORDER BY execution LIMIT 1";

/// The model each replay ran `node_name` on (its first execution, else its `--model` override,
/// else its recorded runtime), distinct, in run order.
fn replay_models(
    conn: &Connection,
    pb_root: &Path,
    members: &[&RunRow],
    node_name: &str,
) -> Vec<String> {
    let mut models: Vec<String> = Vec::new();
    for run in members {
        let model = conn
            .query_row(
                REPLAY_MODEL_SQL,
                rusqlite::params![run.run_id, node_name],
                |r| r.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
            .or_else(|| run.model_override.clone())
            .or_else(|| crate::harness::runtime_model(pb_root, &run.run_id, node_name));
        if let Some(m) = model.filter(|m| !models.contains(m)) {
            models.push(m);
        }
    }
    models
}

/// pb's own record of what a replay started from.
fn replay_json(pb_root: &Path, run_id: &str) -> Option<Value> {
    if !is_plain_name(run_id) {
        return None;
    }
    let text = std::fs::read_to_string(paths::pb_replay_json(pb_root, run_id)).ok()?;
    serde_json::from_str(&text).ok()
}

/// `build#1` → `build`.
fn node_name_of(node_id: &str) -> &str {
    node_id.split('#').next().unwrap_or(node_id)
}
