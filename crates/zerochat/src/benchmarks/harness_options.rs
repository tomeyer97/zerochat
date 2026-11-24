//! The Start-a-batch form's options, read from pb.toml.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
use ts_rs::TS;

use super::lists::push_unique;

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskSet {
    pub name: String,
    pub tasks: Vec<String>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessOptions {
    pub task_sets: Vec<TaskSet>,
    /// Every task id referenced by a task set, sorted and deduplicated.
    pub tasks: Vec<String>,
    /// `tasks_dir/<id>/task.toml` (repo-history tasks), whether or not a task set names them.
    pub repo_tasks: Vec<String>,
    /// `harness/variants/<name>/` directories that have both a `graph.json` and a
    /// `runtime.json` (ready to pass straight to `pb run --graph/--runtime`).
    pub variants: Vec<String>,
    /// The models pb.toml prices (`[pricing."<model>"]`), sorted: the model override offers
    /// these, still accepting anything typed.
    pub models: Vec<String>,
    /// pb.toml's `[harness] graph` / `runtime` (relative to `pb_root`), for the form's preview
    /// of the default harness.
    pub default_graph: String,
    pub default_runtime: String,
    /// Graph and runtime files under `pb_root/harness`, for the custom-harness picker.
    pub harness_files: crate::harness::HarnessFiles,
    pub default_max_cost_usd: f64,
    pub default_max_wall_minutes: f64,
    pub default_max_steps: i64,
    /// pb.toml's `[limits] max_concurrent_runs` (pb's default 4 when absent): how many runs pb
    /// lets execute at once on this host, across every batch. The form's Parallel defaults to it.
    pub max_concurrent_runs: i64,
}

/// pb's own defaults for `[limits]`, copied from pb: used when pb.toml leaves a key out, so the
/// form shows what pb would use. Keep them in step with pb's.
const PB_DEFAULT_MAX_COST_USD: f64 = 5.0;
const PB_DEFAULT_MAX_WALL_MINUTES: f64 = 45.0;
const PB_DEFAULT_MAX_STEPS: i64 = 12;
const PB_DEFAULT_MAX_CONCURRENT_RUNS: i64 = 4;

/// Reads `pb_root/pb.toml` for the Start-a-batch form's choices: task sets, repo tasks, harness
/// variants and pb's own default limits — so the form never needs to shell out to pb just to
/// populate itself.
pub fn harness_options(pb_root: &Path) -> Result<HarnessOptions> {
    let toml_path = pb_root.join("pb.toml");
    let text = std::fs::read_to_string(&toml_path)
        .with_context(|| format!("cannot read {}", toml_path.display()))?;
    let doc: toml::Value = toml::from_str(&text)
        .with_context(|| format!("{} is not valid TOML", toml_path.display()))?;

    let task_sets = read_task_sets(&doc);
    let mut tasks: Vec<String> = Vec::new();
    for task in task_sets.iter().flat_map(|set| &set.tasks) {
        push_unique(&mut tasks, task);
    }
    tasks.sort();
    let limits = read_limits(&doc);
    let (default_graph, default_runtime) = crate::harness::default_paths(pb_root);

    Ok(HarnessOptions {
        task_sets,
        tasks,
        repo_tasks: scan_repo_tasks(pb_root, &doc),
        variants: scan_variants(pb_root),
        models: priced_models(&doc),
        default_graph,
        default_runtime,
        harness_files: crate::harness::list_harness_files(pb_root),
        default_max_cost_usd: limits.max_cost_usd,
        default_max_wall_minutes: limits.max_wall_minutes,
        default_max_steps: limits.max_steps,
        max_concurrent_runs: limits.max_concurrent_runs,
    })
}

/// `[task_sets]`, by name.
fn read_task_sets(doc: &toml::Value) -> Vec<TaskSet> {
    let Some(table) = doc.get("task_sets").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    let mut names: Vec<&String> = table.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let ids = table[name].as_array().into_iter().flatten();
            let tasks = ids.filter_map(|v| v.as_str().map(str::to_owned)).collect();
            TaskSet {
                name: name.clone(),
                tasks,
            }
        })
        .collect()
}

/// The directories of `[paths] tasks_dir` (default `tasks`) that have a `task.toml`, sorted.
fn scan_repo_tasks(pb_root: &Path, doc: &toml::Value) -> Vec<String> {
    let tasks_dir = doc
        .get("paths")
        .and_then(|p| p.get("tasks_dir"))
        .and_then(toml::Value::as_str)
        .unwrap_or("tasks");
    subdirectories_with(&pb_root.join(tasks_dir), &["task.toml"])
}

/// The directories of `harness/variants` that have both a `graph.json` and a `runtime.json`.
fn scan_variants(pb_root: &Path) -> Vec<String> {
    subdirectories_with(
        &pb_root.join("harness").join("variants"),
        &["graph.json", "runtime.json"],
    )
}

/// The names of `dir`'s subdirectories that have every one of `files`, sorted. Empty when `dir`
/// cannot be read.
fn subdirectories_with(dir: &Path, files: &[&str]) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| {
            let path = entry.path();
            path.is_dir() && files.iter().all(|f| path.join(f).is_file())
        })
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// The models pb.toml prices (`[pricing."<model>"]`), sorted.
fn priced_models(doc: &toml::Value) -> Vec<String> {
    let pricing = doc.get("pricing").and_then(toml::Value::as_table);
    let mut models: Vec<String> = pricing
        .map(|t| t.keys().cloned().collect())
        .unwrap_or_default();
    models.sort();
    models
}

struct PbLimits {
    max_cost_usd: f64,
    max_wall_minutes: f64,
    max_steps: i64,
    max_concurrent_runs: i64,
}

/// `[limits]`, each key pb's default when absent. The cost and wall-time limits are read only
/// when written as TOML floats: `max_cost_usd = 6` reads as the default.
fn read_limits(doc: &toml::Value) -> PbLimits {
    let limits = doc.get("limits");
    let get = |key: &str| limits.and_then(|l| l.get(key));
    let float = |key: &str| get(key).and_then(toml::Value::as_float);
    let integer = |key: &str| get(key).and_then(toml::Value::as_integer);
    PbLimits {
        max_cost_usd: float("max_cost_usd").unwrap_or(PB_DEFAULT_MAX_COST_USD),
        max_wall_minutes: float("max_wall_minutes").unwrap_or(PB_DEFAULT_MAX_WALL_MINUTES),
        max_steps: integer("max_steps").unwrap_or(PB_DEFAULT_MAX_STEPS),
        max_concurrent_runs: integer("max_concurrent_runs")
            .filter(|n| *n >= 1)
            .unwrap_or(PB_DEFAULT_MAX_CONCURRENT_RUNS),
    }
}
