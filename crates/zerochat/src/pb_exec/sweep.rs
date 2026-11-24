//! `pb sweep-report`: what pb's model sweeps measured and which model each graph node should use.
//!
//! A sweep replays one recorded run from a node onwards with several models per node; the report
//! pools those replays per measured node (or node→through segment), compares each model with the
//! recorded one (the baseline), recommends a model per node and suggests a whole-run mapping,
//! optionally under a per-run cost budget.
//!
//! The types are deserialised from pb's snake_case `sweep.report()` output (pb `sweep.py`) and
//! re-serialised in camelCase for `web/src/types.ts` `SweepReport`. Field names must match pb's
//! dict keys.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::{pb_error, run};

/// One model's replays of one measured node, pooled over the starting states it was replayed
/// from. The deltas are paired: per starting state, this model's mean minus the baseline's.
#[derive(Deserialize, Serialize, Clone, Default, TS)]
#[serde(rename_all(serialize = "camelCase"))]
#[ts(rename_all = "camelCase")]
pub struct SweepModelStat {
    pub model: String,
    /// The model the recorded run used at this node.
    pub is_baseline: bool,
    /// Scored replays.
    pub n: i64,
    /// All replays, scored or not.
    pub replays: i64,
    /// Starting states with scored replays of both this model and the baseline.
    pub states: i64,
    /// Mean, lowest and highest score.
    pub mean: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Mean cost of the node itself, and of the whole replay from it, in USD.
    pub node_cost: Option<f64>,
    pub seg_cost: Option<f64>,
    /// Score change against the baseline.
    pub delta: Option<f64>,
    /// The noise band: the larger min–max score range of this model and the baseline.
    pub spread: Option<f64>,
    /// Segment cost change against the baseline (USD).
    pub dcost: Option<f64>,
    /// Node cost change against the baseline (USD).
    pub dnode: Option<f64>,
    /// pb's verdict on `delta`: `baseline`, `within noise`, `beyond noise`, `no baseline` or
    /// `unknown (n<2)`.
    pub noise: String,
    /// How many replays ended with each verdict at this node.
    pub verdicts: HashMap<String, i64>,
    /// `delta` per extra USD of segment cost (none when the model costs no more).
    pub per_usd: Option<f64>,
}

/// pb's advice for one measured node.
#[derive(Deserialize, Serialize, Clone, Default, TS)]
#[serde(rename_all(serialize = "camelCase"))]
#[ts(rename_all = "camelCase")]
pub struct SweepRecommendation {
    /// `upgrade`, `downgrade OK`, `keep` or `unknown`.
    pub action: String,
    pub model: Option<String>,
    /// The recommended model's score and segment cost change (absent for `unknown`).
    #[serde(default)]
    pub delta: Option<f64>,
    #[serde(default)]
    pub dcost: Option<f64>,
    /// The advice in words, as `pb sweep-report` prints it.
    pub text: String,
}

/// Everything measured for one node (or node→through segment), every model compared.
#[derive(Deserialize, Serialize, Clone, Default, TS)]
#[serde(rename_all(serialize = "camelCase"))]
#[ts(rename_all = "camelCase")]
pub struct SweepAllocation {
    /// `node`, or `node→through` for a segment.
    pub key: String,
    pub node: String,
    pub through: Option<String>,
    /// The recorded models (baselines) this node was replayed against.
    pub baselines: Vec<String>,
    /// Baselines first.
    pub models: Vec<SweepModelStat>,
    pub states: i64,
    pub tasks: Vec<String>,
    pub source_runs: Vec<String>,
    pub recommendation: SweepRecommendation,
}

/// One node's model change in a suggested mapping.
#[derive(Deserialize, Serialize, Clone, Default, TS)]
#[serde(rename_all(serialize = "camelCase"))]
#[ts(rename_all = "camelCase")]
pub struct SweepMappingChange {
    pub node: String,
    pub from: Option<String>,
    pub to: Option<String>,
    /// `upgrade`, `downgrade OK` or `upgrade (budget)`.
    pub why: String,
    /// The measured key the change is based on.
    pub key: String,
    /// Expected score change.
    pub delta: f64,
    /// Expected cost change per run: the segment cost change times `execs`.
    pub dcost_run: f64,
    /// How many times the node runs in a recorded run, on average (more than once in a loop).
    pub execs: f64,
}

/// A whole-run mapping node → model, and what it is expected to change.
#[derive(Deserialize, Serialize, Clone, Default, TS)]
#[serde(rename_all(serialize = "camelCase"))]
#[ts(rename_all = "camelCase")]
pub struct SweepSuggestion {
    pub mapping: HashMap<String, Option<String>>,
    pub changes: Vec<SweepMappingChange>,
    pub delta_score: f64,
    pub delta_cost: f64,
    /// The recorded runs' mean cost, and the estimate with the changes applied (USD per run).
    pub base_cost: Option<f64>,
    pub est_cost: Option<f64>,
    pub budget: Option<f64>,
    pub over_budget: bool,
}

/// The recorded runs the sweeps replayed.
#[derive(Deserialize, Serialize, Clone, Default, TS)]
#[serde(rename_all(serialize = "camelCase"))]
#[ts(rename_all = "camelCase")]
pub struct SweepSource {
    pub runs: Vec<String>,
    pub mean_cost: Option<f64>,
    /// Mean executions per node name per run.
    pub execs: HashMap<String, f64>,
}

/// The whole `pb sweep-report --json` output for the selected sweeps.
#[derive(Deserialize, Serialize, Clone, Default, TS)]
#[serde(rename_all(serialize = "camelCase"))]
#[ts(rename_all = "camelCase")]
pub struct SweepReport {
    /// Replays read.
    pub rows: i64,
    pub allocation: Vec<SweepAllocation>,
    pub source: SweepSource,
    /// The newest source run's runtime (the template `--write-runtime` edits).
    #[ts(type = "Json | null")]
    pub runtime: Option<Value>,
    /// The model the recorded runtime gives each agent node.
    pub recorded: HashMap<String, Option<String>>,
    pub suggestion: SweepSuggestion,
    /// The suggestion under `--budget`, when one was given.
    pub budgeted: Option<SweepSuggestion>,
    pub caveat: String,
    /// pb itself writes this key in camelCase.
    #[serde(default, rename = "writtenRuntimeFile")]
    pub written_runtime_file: Option<String>,
}

/// The filters and outputs of one `pb sweep-report` call. Empty strings count as unset.
#[derive(Default, Clone, Copy, Debug)]
pub struct SweepReportOptions<'a> {
    pub label: Option<&'a str>,
    pub task: Option<&'a str>,
    pub budget: Option<f64>,
    /// Where pb writes the suggested runtime, relative to `pb_root`.
    pub write_runtime: Option<&'a str>,
}

pub fn sweep_report_args(sweep_ids: &[String], options: &SweepReportOptions) -> Vec<String> {
    let set = |value: Option<&str>| value.filter(|v| !v.is_empty()).map(str::to_owned);
    let mut args = vec!["sweep-report".to_owned()];
    args.extend(sweep_ids.iter().cloned());
    let flags = [
        ("--label", set(options.label)),
        ("--task", set(options.task)),
        ("--budget", options.budget.map(|b| b.to_string())),
        ("--write-runtime", set(options.write_runtime)),
    ];
    for (flag, value) in flags {
        if let Some(value) = value {
            args.extend([flag.to_owned(), value]);
        }
    }
    args.push("--json".into());
    args
}

/// [`sweep_report_with`], options passed one by one.
#[allow(clippy::too_many_arguments)]
pub async fn sweep_report(
    pb_root: &Path,
    pb_command: &str,
    sweep_ids: &[String],
    label: Option<&str>,
    task: Option<&str>,
    budget: Option<f64>,
    write_runtime: Option<&str>,
) -> Result<Value> {
    sweep_report_with(
        pb_root,
        pb_command,
        sweep_ids,
        &SweepReportOptions {
            label,
            task,
            budget,
            write_runtime,
        },
    )
    .await
}

/// Runs `pb sweep-report --json` and returns its report in the web's camelCase shape.
pub async fn sweep_report_with(
    pb_root: &Path,
    pb_command: &str,
    sweep_ids: &[String],
    options: &SweepReportOptions<'_>,
) -> Result<Value> {
    let stdout = run(pb_root, pb_command, &sweep_report_args(sweep_ids, options)).await?;
    let report: SweepReport = serde_json::from_str(stdout.trim()).map_err(|error| {
        let preview: String = stdout.chars().take(300).collect();
        pb_error(format!(
            "pb sweep-report --json did not print valid JSON: {error} ({preview})"
        ))
    })?;
    serde_json::to_value(report)
        .map_err(|error| pb_error(format!("could not re-encode pb's sweep report: {error}")))
}
