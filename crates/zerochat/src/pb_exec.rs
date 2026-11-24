//! Shells out to `pb` for the actions its own code must perform: starting and stopping a batch
//! (`pb run --detach`, `pb stop`), the host's run slots (`pb slots`) and `pb sweep-report`'s
//! allocation math (in [`sweep`]). Unlike the read-only queries in `benchmarks`, which read
//! `pb.sqlite` directly, these need pb's own code to run (a sandboxed benchmark launch, or math
//! zerochat would otherwise have to duplicate and keep in sync). `pb_command`
//! (`config::Config::pb_command`) names the invocation; the process always runs with its current
//! directory set to `pb_root`, so pb's own `pb.toml` search (which walks up from the current
//! directory) finds the right project even when `pb_command` names a different checkout's
//! environment.

pub mod sweep;

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;
use ts_rs::TS;

pub use sweep::sweep_report;
// Named from here by the generated web types (`api_types`) and the tests below.
#[allow(unused_imports)]
pub use sweep::{
    SweepAllocation, SweepMappingChange, SweepModelStat, SweepRecommendation, SweepReport,
    SweepReportOptions, SweepSource, SweepSuggestion, sweep_report_args,
};

/// A `pb` invocation failed (a non-zero exit, or it could not be started at all). The message is
/// pb's own output, shown to the user verbatim (same pattern as `actions::CliError`).
#[derive(Debug)]
pub struct PbCommandError {
    pub message: String,
}

impl std::fmt::Display for PbCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PbCommandError {}

fn pb_error(message: impl Into<String>) -> anyhow::Error {
    anyhow!(PbCommandError {
        message: message.into()
    })
}

/// `pb_command` split on whitespace. No quoting support: a path containing a space needs a
/// wrapper script named by `pb_command` instead.
fn tokenize(pb_command: &str) -> Vec<String> {
    pb_command.split_whitespace().map(str::to_owned).collect()
}

/// Runs `pb_command args...` in `pb_root` and returns its stdout. On a non-zero exit the error is
/// pb's own output (stdout, else stderr, else the exit status).
async fn run(pb_root: &Path, pb_command: &str, args: &[String]) -> Result<String> {
    let mut tokens = tokenize(pb_command);
    if tokens.is_empty() {
        bail!(PbCommandError {
            message: "pb_command is empty.".to_owned()
        });
    }
    let program = tokens.remove(0);
    let mut command = Command::new(&program);
    command
        .args(&tokens)
        .args(args)
        .current_dir(pb_root)
        .kill_on_drop(true);
    let output = command
        .output()
        .await
        .map_err(|error| pb_error(format!("cannot start `{program}`: {error}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        return Ok(stdout);
    }
    let message = [stdout, stderr]
        .into_iter()
        .map(|s| s.trim().to_owned())
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| format!("pb exited with {}", output.status));
    Err(pb_error(message))
}

// ---------- start a batch ----------

#[derive(Deserialize, Default, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Harness {
    #[default]
    #[serde(rename = "default")]
    Stock,
    Variant {
        name: String,
    },
    Custom {
        graph: String,
        runtime: String,
    },
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StartBatchRequest {
    /// pb's own `--tasks` syntax: comma-separated task-set names and/or task ids (the web form
    /// resolves set names to ids itself for its spend preview, but sends pb's own syntax so pb
    /// resolves it the same way `pb run --tasks` always has).
    pub tasks: String,
    pub seeds: u32,
    pub label: String,
    #[serde(default)]
    pub harness: Harness,
    pub model: Option<String>,
    pub max_cost_usd: Option<f64>,
    pub max_minutes: Option<f64>,
    pub max_steps: Option<u32>,
    pub parallel: Option<u32>,
}

/// The `pb run --detach ...` argument list for this request, or a message to show the user
/// instead of starting anything (validation only; no process is touched). The first refusal
/// wins, in the order the arguments are built.
pub fn start_batch_args(req: &StartBatchRequest, pb_root: &Path) -> Result<Vec<String>, String> {
    if req.tasks.trim().is_empty() {
        return Err("Pick at least one task set or task.".to_owned());
    }
    if req.label.trim().is_empty() {
        return Err("A label is required.".to_owned());
    }
    if req.seeds < 1 {
        return Err("Seeds must be at least 1.".to_owned());
    }
    let mut args: Vec<String> = vec![
        "run".into(),
        "--detach".into(),
        "--tasks".into(),
        req.tasks.clone(),
        "--seeds".into(),
        req.seeds.to_string(),
        "--label".into(),
        req.label.clone(),
    ];
    if let Some(model) = req.model.as_deref().filter(|m| !m.trim().is_empty()) {
        args.extend(["--model".into(), model.to_owned()]);
    }
    push_positive(
        &mut args,
        "--max-cost",
        req.max_cost_usd,
        "Cost cap must be a positive dollar amount.",
    )?;
    push_positive(
        &mut args,
        "--max-minutes",
        req.max_minutes,
        "Wall-time cap must be positive.",
    )?;
    push_positive(
        &mut args,
        "--max-steps",
        req.max_steps,
        "Step cap must be positive.",
    )?;
    push_positive(
        &mut args,
        "--parallel",
        req.parallel,
        "Parallelism must be at least 1.",
    )?;
    args.extend(harness_args(&req.harness, pb_root)?);
    Ok(args)
}

/// Appends `flag value` when `value` is set; a zero or negative value is refused with `refusal`.
fn push_positive<T: PartialOrd + Default + ToString>(
    args: &mut Vec<String>,
    flag: &str,
    value: Option<T>,
    refusal: &str,
) -> Result<(), String> {
    let Some(value) = value else { return Ok(()) };
    if value <= T::default() {
        return Err(refusal.to_owned());
    }
    args.extend([flag.to_owned(), value.to_string()]);
    Ok(())
}

/// `--graph <file> --runtime <file>` for a variant or custom harness (nothing for pb's default).
/// Both files are checked here rather than letting pb's own `open(path)` fail with a raw Python
/// `[Errno 2] No such file or directory` that would reach the user verbatim.
fn harness_args(harness: &Harness, pb_root: &Path) -> Result<Vec<String>, String> {
    let (graph, runtime) = match harness {
        Harness::Stock => return Ok(Vec::new()),
        Harness::Variant { name } => {
            if !crate::validate::is_plain_name(name) {
                return Err("Invalid harness variant name.".to_owned());
            }
            let variant = crate::harness::variant(pb_root, name);
            if !variant.is_complete() {
                return Err(format!(
                    "harness/variants/{name} has no graph.json and runtime.json."
                ));
            }
            (variant.graph(), variant.runtime())
        }
        Harness::Custom { graph, runtime } => {
            if graph.trim().is_empty() || runtime.trim().is_empty() {
                return Err("A custom harness needs both a graph and a runtime file.".to_owned());
            }
            // Inside `pb_root` only, and the refusal names only what was sent, never the
            // server's own directory.
            let graph = crate::harness::resolve_in_root(pb_root, graph)
                .map_err(|e| format!("Graph: {e}"))?;
            let runtime = crate::harness::resolve_in_root(pb_root, runtime)
                .map_err(|e| format!("Runtime: {e}"))?;
            (graph, runtime)
        }
    };
    Ok(vec![
        "--graph".into(),
        graph.display().to_string(),
        "--runtime".into(),
        runtime.display().to_string(),
    ])
}

/// What `pb run --detach` prints when it started a batch (pb has no `--json` for this command):
/// the batch id, then the detached runner's pid. Rich strips its own markup before this text
/// reaches a pipe, so no `[cyan]`-style tags survive. Its refusal (another batch already
/// running) has no such line.
///
/// `batch 20261105-143200-ab12cd (detached, pid 4242; log /x/batches/20261105-143200-ab12cd/pb.log)`
const BATCH_LINE_PREFIX: &str = "batch ";
/// Where the pid starts in that same line.
const DETACHED_PID_MARKER: &str = "detached, pid ";

/// The batch id from `pb run --detach`'s success line (see [`BATCH_LINE_PREFIX`]).
pub fn parse_batch_id(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        line.trim()
            .strip_prefix(BATCH_LINE_PREFIX)?
            .split_whitespace()
            .next()
            .map(str::to_owned)
    })
}

/// The detached runner's pid, from the same line (see [`BATCH_LINE_PREFIX`]). `pb run --detach`
/// returns before that process has written the batch's `batches` row, so for a few seconds (or
/// for good, if it dies first) the id it printed names nothing in pb.sqlite; this pid tells
/// "still starting" apart from "died before it started".
pub fn parse_detached_pid(stdout: &str) -> Option<u32> {
    let rest = stdout.split(DETACHED_PID_MARKER).nth(1)?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// A batch `pb run --detach` just started: its id, and the detached runner's pid when pb said.
#[derive(Debug, Clone, PartialEq)]
pub struct StartedBatch {
    pub id: String,
    pub pid: Option<u32>,
}

pub async fn start_batch(
    pb_root: &Path,
    pb_command: &str,
    req: &StartBatchRequest,
) -> Result<StartedBatch> {
    let args = start_batch_args(req, pb_root).map_err(pb_error)?;
    let stdout = run(pb_root, pb_command, &args).await?;
    let id = parse_batch_id(&stdout)
        .ok_or_else(|| pb_error(format!("pb did not report a batch id: {}", stdout.trim())))?;
    Ok(StartedBatch {
        id,
        pid: parse_detached_pid(&stdout),
    })
}

// ---------- stop a batch ----------

pub async fn stop_batch(pb_root: &Path, pb_command: &str, id: &str) -> Result<String> {
    if id.starts_with("label:") {
        bail!(PbCommandError {
            message: "This is a pseudo-batch (runs grouped by label, from before pb's batches table existed); there is no pb process to stop.".to_owned()
        });
    }
    run(pb_root, pb_command, &["stop".to_owned(), id.to_owned()]).await
}

// ---------- run slots ----------

/// `pb slots --json`: the host-wide run-slot picture (every pb process and checkout on this host
/// shares `[limits] max_concurrent_runs` slots), reduced to the counts the Start form shows.
#[derive(Serialize, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunSlots {
    pub max_concurrent_runs: Option<i64>,
    pub running: usize,
    pub queued: usize,
}

pub fn parse_run_slots(stdout: &str) -> Result<RunSlots> {
    let v: Value = serde_json::from_str(stdout.trim())
        .map_err(|error| pb_error(format!("pb slots --json did not print valid JSON: {error}")))?;
    let count = |k: &str| v.get(k).and_then(Value::as_array).map_or(0, Vec::len);
    Ok(RunSlots {
        max_concurrent_runs: v.get("max_concurrent_runs").and_then(Value::as_i64),
        running: count("running"),
        queued: count("waiting"),
    })
}

pub async fn run_slots(pb_root: &Path, pb_command: &str) -> Result<RunSlots> {
    parse_run_slots(
        &run(
            pb_root,
            pb_command,
            &["slots".to_owned(), "--json".to_owned()],
        )
        .await?,
    )
}

/// The types of this module the web reads (`web/src/generated/pb_exec.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, pb_exec: RunSlots, SweepModelStat, SweepRecommendation, SweepAllocation, SweepMappingChange,
        SweepSuggestion, SweepSource, SweepReport)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(tasks: &str, label: &str) -> StartBatchRequest {
        StartBatchRequest {
            tasks: tasks.to_owned(),
            seeds: 2,
            label: label.to_owned(),
            harness: Harness::Stock,
            model: None,
            max_cost_usd: None,
            max_minutes: None,
            max_steps: None,
            parallel: None,
        }
    }

    #[test]
    fn default_harness_builds_the_plain_detach_command() {
        let args = start_batch_args(&req("dev,extra-task", "try-x"), Path::new("/pb")).unwrap();
        assert_eq!(
            args,
            vec![
                "run",
                "--detach",
                "--tasks",
                "dev,extra-task",
                "--seeds",
                "2",
                "--label",
                "try-x"
            ]
        );
    }

    #[test]
    fn empty_tasks_or_label_or_zero_seeds_are_rejected_before_anything_runs() {
        assert!(start_batch_args(&req("", "x"), Path::new("/pb")).is_err());
        assert!(start_batch_args(&req("dev", ""), Path::new("/pb")).is_err());
        let mut r = req("dev", "x");
        r.seeds = 0;
        assert!(start_batch_args(&r, Path::new("/pb")).is_err());
    }

    #[test]
    fn limits_and_model_and_parallel_are_appended_when_set() {
        let mut r = req("dev", "x");
        r.model = Some("claude-haiku-4-5".into());
        r.max_cost_usd = Some(2.5);
        r.max_minutes = Some(30.0);
        r.max_steps = Some(10);
        r.parallel = Some(3);
        let args = start_batch_args(&r, Path::new("/pb")).unwrap();
        assert_eq!(
            args,
            vec![
                "run",
                "--detach",
                "--tasks",
                "dev",
                "--seeds",
                "2",
                "--label",
                "x",
                "--model",
                "claude-haiku-4-5",
                "--max-cost",
                "2.5",
                "--max-minutes",
                "30",
                "--max-steps",
                "10",
                "--parallel",
                "3"
            ]
        );
    }

    #[test]
    fn a_zero_or_negative_limit_is_rejected() {
        let mut r = req("dev", "x");
        r.max_cost_usd = Some(0.0);
        assert!(start_batch_args(&r, Path::new("/pb")).is_err());
        let mut r2 = req("dev", "x");
        r2.max_steps = Some(0);
        assert!(start_batch_args(&r2, Path::new("/pb")).is_err());
    }

    #[test]
    fn variant_harness_needs_graph_and_runtime_files_present() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = req("dev", "x");
        r.harness = Harness::Variant {
            name: "clone-v1".into(),
        };
        assert!(
            start_batch_args(&r, dir.path()).is_err(),
            "no harness/variants/clone-v1 yet"
        );

        let vdir = dir.path().join("harness/variants/clone-v1");
        std::fs::create_dir_all(&vdir).unwrap();
        std::fs::write(vdir.join("graph.json"), "{}").unwrap();
        std::fs::write(vdir.join("runtime.json"), "{}").unwrap();
        let args = start_batch_args(&r, dir.path()).unwrap();
        assert!(args.contains(&"--graph".to_owned()));
        assert_eq!(
            args[args.iter().position(|a| a == "--graph").unwrap() + 1],
            vdir.join("graph.json").display().to_string()
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--runtime").unwrap() + 1],
            vdir.join("runtime.json").display().to_string()
        );
    }

    #[test]
    fn variant_name_cannot_escape_the_variants_directory() {
        let mut r = req("dev", "x");
        r.harness = Harness::Variant {
            name: "../../etc".into(),
        };
        assert!(start_batch_args(&r, Path::new("/pb")).is_err());
    }

    #[test]
    fn custom_harness_resolves_relative_paths_under_pb_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("my")).unwrap();
        std::fs::write(dir.path().join("my/graph.json"), "{}").unwrap();
        let abs_runtime = dir.path().join("abs-runtime.json");
        std::fs::write(&abs_runtime, "{}").unwrap();
        let mut r = req("dev", "x");
        r.harness = Harness::Custom {
            graph: "my/graph.json".into(),
            runtime: abs_runtime.display().to_string(),
        };
        let args = start_batch_args(&r, dir.path()).unwrap();
        let graph_i = args.iter().position(|a| a == "--graph").unwrap() + 1;
        let runtime_i = args.iter().position(|a| a == "--runtime").unwrap() + 1;
        assert_eq!(
            args[graph_i],
            dir.path()
                .join("my/graph.json")
                .canonicalize()
                .unwrap()
                .display()
                .to_string()
        );
        assert_eq!(
            args[runtime_i],
            abs_runtime.canonicalize().unwrap().display().to_string()
        ); // absolute, inside pb_root: fine
    }

    /// A custom harness must stay inside the pb project, and the refusal names only what was
    /// sent, never the server's own directory.
    #[test]
    fn custom_harness_outside_pb_root_is_refused_without_leaking_the_server_path() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("g.json"), "{}").unwrap();
        std::fs::write(dir.path().join("r.json"), "{}").unwrap();
        let mut r = req("dev", "x");
        r.harness = Harness::Custom {
            graph: "../g.json".into(),
            runtime: "r.json".into(),
        };
        let error = start_batch_args(&r, dir.path()).unwrap_err();
        assert!(
            !error.contains(&dir.path().display().to_string()),
            "{error}"
        );
        r.harness = Harness::Custom {
            graph: outside.path().join("g.json").display().to_string(),
            runtime: "r.json".into(),
        };
        let error = start_batch_args(&r, dir.path()).unwrap_err();
        assert!(error.contains("outside the pb project"), "{error}");
        assert!(
            !error.contains(&dir.path().display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn custom_harness_needs_both_paths() {
        let mut r = req("dev", "x");
        r.harness = Harness::Custom {
            graph: "g.json".into(),
            runtime: "".into(),
        };
        assert!(start_batch_args(&r, Path::new("/pb")).is_err());
    }

    /// A missing custom-harness file must not reach pb, which would raise a raw Python
    /// `[Errno 2] No such file or directory` straight back to the user; checked here instead,
    /// the same way the variant case is.
    #[test]
    fn custom_harness_with_a_missing_file_is_rejected_before_pb_runs() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = req("dev", "x");
        r.harness = Harness::Custom {
            graph: "nope/graph.json".into(),
            runtime: "nope/runtime.json".into(),
        };
        let error = start_batch_args(&r, dir.path()).unwrap_err();
        assert!(error.contains("File not found"), "{error}");
        assert!(error.contains("nope/graph.json"), "{error}");
    }

    #[test]
    fn harness_kind_json_tags_match_the_web_form() {
        let stock: Harness = serde_json::from_str(r#"{"kind":"default"}"#).unwrap();
        assert_eq!(stock, Harness::Stock);
        let variant: Harness =
            serde_json::from_str(r#"{"kind":"variant","name":"ticket-v1"}"#).unwrap();
        assert_eq!(
            variant,
            Harness::Variant {
                name: "ticket-v1".into()
            }
        );
        let custom: Harness =
            serde_json::from_str(r#"{"kind":"custom","graph":"g","runtime":"r"}"#).unwrap();
        assert_eq!(
            custom,
            Harness::Custom {
                graph: "g".into(),
                runtime: "r".into()
            }
        );
    }

    #[test]
    fn batch_id_is_parsed_from_pbs_own_success_line() {
        let out = "harness version v1 (commit abc, zeroshot 11.2.0)\nbatch 20261105-143200-ab12cd (detached, pid 4242; log /x/batches/20261105-143200-ab12cd/pb.log)\n";
        assert_eq!(
            parse_batch_id(out),
            Some("20261105-143200-ab12cd".to_owned())
        );
        assert_eq!(parse_detached_pid(out), Some(4242));
        assert_eq!(parse_detached_pid("batch x started\n"), None);
    }

    #[test]
    fn no_batch_id_when_pb_refused_instead_of_starting() {
        let out = "batch running-batch-x is already running (pid 123 on host); only one pb batch runs at a time. `pb stop running-batch-x` to stop it first.\n";
        // Still matches "batch " textually, but the token after it is not an id pb created now.
        // The caller treats whatever token follows as the id only on a zero exit status; a non-zero
        // exit never reaches parse_batch_id (see `start_batch`).
        assert_eq!(parse_batch_id(out), Some("running-batch-x".to_owned()));
    }

    #[test]
    fn stopping_a_pseudo_batch_is_refused_before_any_process_runs() {
        let result = block_on_stop_pseudo();
        assert!(result.is_err());
    }

    fn block_on_stop_pseudo() -> Result<String, anyhow::Error> {
        // No process is spawned for a pseudo id, so this can run on a plain (non-tokio) test thread
        // by driving the future to completion with a minimal executor.
        futures::executor::block_on(stop_batch(
            Path::new("/does/not/exist"),
            "/does/not/exist/pb",
            "label:baseline",
        ))
    }

    #[test]
    fn run_slots_counts_holders_and_waiters_from_pbs_json() {
        let out = r#"{"max_concurrent_runs": 4, "running": [{"slot": "slot-0", "task": "t", "batch_id": "b1"}, {"slot": "slot-2"}], "waiting": [{"task": "t"}]}"#;
        assert_eq!(
            parse_run_slots(out).unwrap(),
            RunSlots {
                max_concurrent_runs: Some(4),
                running: 2,
                queued: 1
            }
        );
        assert!(
            parse_run_slots("Usage: pb [OPTIONS] COMMAND").is_err(),
            "an older pb without `pb slots`"
        );
    }

    #[test]
    fn sweep_report_args_include_filters_and_always_end_in_json() {
        let options = SweepReportOptions {
            label: Some("L"),
            task: Some("gron"),
            budget: Some(4.0),
            write_runtime: Some("out.json"),
        };
        let args = sweep_report_args(&["sw-1".into(), "sw-2".into()], &options);
        assert_eq!(
            args,
            vec![
                "sweep-report",
                "sw-1",
                "sw-2",
                "--label",
                "L",
                "--task",
                "gron",
                "--budget",
                "4",
                "--write-runtime",
                "out.json",
                "--json"
            ]
        );
        assert_eq!(
            sweep_report_args(&[], &SweepReportOptions::default()),
            vec!["sweep-report", "--json"]
        );
    }

    /// pb's own `sweep-report --json` prints snake_case (`node_cost`, `seg_cost`, `is_baseline`,
    /// `delta_score`, `over_budget`, ...); the web UI reads camelCase. Every key the UI reads
    /// must survive the round trip through `SweepReport` with its value intact, including a
    /// recommendation whose `delta`/`dcost` keys are absent from pb's own JSON (pb's "unknown"
    /// recommendation never sets them) and a budgeted mapping that is genuinely over budget.
    #[test]
    fn sweep_report_round_trips_pbs_snake_case_json_into_the_camel_case_the_ui_reads() {
        let pb_json = serde_json::json!({
            "rows": 8,
            "allocation": [{
                "key": "triage→repair", "node": "triage", "through": "repair",
                "baselines": ["claude-haiku-4-5"],
                "models": [
                    {"model": "claude-haiku-4-5", "is_baseline": true, "n": 4, "replays": 4, "states": 1,
                     "mean": 0.61, "min": 0.58, "max": 0.64, "node_cost": 0.1475, "seg_cost": 1.2052,
                     "delta": null, "spread": null, "dcost": null, "dnode": null, "noise": "baseline",
                     "verdicts": {}, "per_usd": null},
                    {"model": "claude-sonnet-5", "is_baseline": false, "n": 4, "replays": 4, "states": 1,
                     "mean": 0.65, "min": 0.60, "max": 0.70, "node_cost": 0.2448, "seg_cost": 1.6677,
                     "delta": 0.024, "spread": 0.06, "dcost": 0.4625, "dnode": 0.0973, "noise": "within noise",
                     "verdicts": {"accept": 2}, "per_usd": 0.0241}
                ],
                "states": 1, "tasks": ["gron"], "source_runs": ["20261103-151748-ef96e3"],
                "recommendation": {"action": "unknown", "model": "claude-haiku-4-5", "text": "unknown (need more replays: ≥2 scored per model on the same starting state)"}
            }],
            "source": {"runs": ["20261103-151748-ef96e3"], "mean_cost": 5.4, "execs": {"triage": 1.0, "repair": 1.0}},
            "runtime": {"nodes": {"triage": {"kind": "agent", "model": "claude-haiku-4-5"}}},
            "recorded": {"triage": "claude-haiku-4-5", "repair": "claude-haiku-4-5"},
            "suggestion": {"mapping": {"triage": "claude-haiku-4-5"}, "changes": [], "delta_score": 0.0, "delta_cost": 0.0,
                           "base_cost": 5.4, "est_cost": 5.4, "budget": null, "over_budget": false},
            "budgeted": {"mapping": {"triage": "claude-haiku-4-5"}, "changes": [], "delta_score": 0.0, "delta_cost": 0.0,
                         "base_cost": 5.4, "est_cost": 5.4, "budget": 3.0, "over_budget": true},
            "caveat": "Recommendations are conditional on the sampled starting states...",
            "writtenRuntimeFile": "harness/runtime.suggested.json"
        });
        let report: SweepReport = serde_json::from_str(&pb_json.to_string()).unwrap();
        let value = serde_json::to_value(&report).unwrap();

        let baseline = &value["allocation"][0]["models"][0];
        assert_eq!(baseline["isBaseline"], true);
        assert_eq!(baseline["nodeCost"], 0.1475);
        assert_eq!(baseline["segCost"], 1.2052);
        assert_eq!(baseline["perUsd"], Value::Null);

        let sonnet = &value["allocation"][0]["models"][1];
        assert_eq!(sonnet["isBaseline"], false);
        assert_eq!(sonnet["nodeCost"], 0.2448);
        assert_eq!(sonnet["segCost"], 1.6677);
        assert_eq!(sonnet["perUsd"], 0.0241);
        assert_eq!(
            value["allocation"][0]["sourceRuns"],
            serde_json::json!(["20261103-151748-ef96e3"])
        );

        // "unknown" never had delta/dcost keys in pb's JSON at all: must come through as null,
        // not a missing-field deserialize error.
        let rec = &value["allocation"][0]["recommendation"];
        assert_eq!(rec["action"], "unknown");
        assert_eq!(rec["delta"], Value::Null);
        assert_eq!(rec["dcost"], Value::Null);

        assert_eq!(value["suggestion"]["deltaScore"], 0.0);
        assert_eq!(value["suggestion"]["baseCost"], 5.4);
        assert_eq!(value["suggestion"]["estCost"], 5.4);
        assert_eq!(value["suggestion"]["overBudget"], false);

        // The budgeted mapping IS over budget: `overBudget` must read true so the UI can warn,
        // instead of silently showing a mapping that costs more than the budget it was asked for.
        assert_eq!(value["budgeted"]["overBudget"], true);
        assert_eq!(value["budgeted"]["budget"], 3.0);

        assert_eq!(value["source"]["meanCost"], 5.4);
        assert_eq!(
            value["writtenRuntimeFile"],
            "harness/runtime.suggested.json"
        );
    }
    /// Every `pb` command line the builders produce for a matrix of Start-form and sweep-report
    /// inputs (each option alone, combined, at its edges, and each refusal). Recorded once from
    /// the builders as they were before their readability refactor, so any change to an argv or
    /// a refusal message shows up here.
    fn argv_matrix() -> serde_json::Value {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let vdir = root.join("harness/variants/clone-v1");
        std::fs::create_dir_all(&vdir).unwrap();
        std::fs::write(vdir.join("graph.json"), "{}").unwrap();
        std::fs::write(vdir.join("runtime.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("harness/variants/graph-only")).unwrap();
        std::fs::write(root.join("harness/variants/graph-only/graph.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("my")).unwrap();
        std::fs::write(root.join("my/graph.json"), "{}").unwrap();
        std::fs::write(root.join("my/runtime.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("my/dir.json")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("g.json"), "{}").unwrap();
        let outside_g = outside.path().join("g.json").display().to_string();
        let abs_g = root.join("my/graph.json").display().to_string();

        let base = || StartBatchRequest {
            tasks: "dev".into(),
            seeds: 2,
            label: "x".into(),
            harness: Harness::Stock,
            model: None,
            max_cost_usd: None,
            max_minutes: None,
            max_steps: None,
            parallel: None,
        };
        let mut cases: Vec<(String, StartBatchRequest)> = Vec::new();
        let mut add = |name: &str, f: &dyn Fn(&mut StartBatchRequest)| {
            let mut r = base();
            f(&mut r);
            cases.push((name.to_owned(), r));
        };
        add("plain", &|_| {});
        add("tasks list", &|r| r.tasks = "dev,extra-task, t3".into());
        add("tasks blank", &|r| r.tasks = "  ".into());
        add("tasks empty", &|r| r.tasks = "".into());
        add("label spaces", &|r| r.label = "my label  x".into());
        add("label blank", &|r| r.label = " ".into());
        add("seeds 0", &|r| r.seeds = 0);
        add("seeds 1", &|r| r.seeds = 1);
        add("seeds 100", &|r| r.seeds = 100);
        add("model", &|r| r.model = Some("claude-haiku-4-5".into()));
        add("model padded", &|r| {
            r.model = Some(" claude-haiku-4-5 ".into())
        });
        add("model empty", &|r| r.model = Some("".into()));
        add("model blank", &|r| r.model = Some("   ".into()));
        for v in [
            2.5,
            0.1,
            0.001,
            30.0,
            1e6,
            0.30000000000000004,
            1e21,
            0.0,
            -1.0,
        ] {
            add(&format!("max_cost {v:?}"), &move |r| {
                r.max_cost_usd = Some(v)
            });
            add(&format!("max_minutes {v:?}"), &move |r| {
                r.max_minutes = Some(v)
            });
        }
        for v in [0u32, 1, 10, u32::MAX] {
            add(&format!("max_steps {v}"), &move |r| r.max_steps = Some(v));
            add(&format!("parallel {v}"), &move |r| r.parallel = Some(v));
        }
        add("all limits", &|r| {
            r.model = Some("m".into());
            r.max_cost_usd = Some(2.5);
            r.max_minutes = Some(30.0);
            r.max_steps = Some(10);
            r.parallel = Some(3);
        });
        add("first error wins: model ok, cost and parallel bad", &|r| {
            r.model = Some("m".into());
            r.max_cost_usd = Some(-2.0);
            r.parallel = Some(0);
        });
        add("first error wins: minutes and steps bad", &|r| {
            r.max_minutes = Some(0.0);
            r.max_steps = Some(0);
        });
        add("limit error before harness error", &|r| {
            r.max_steps = Some(0);
            r.harness = Harness::Variant { name: "..".into() };
        });
        for name in [
            "clone-v1",
            "graph-only",
            "missing",
            "",
            ".hidden",
            "..",
            "a/b",
            "a\\b",
            "../../etc",
        ] {
            add(&format!("variant {name:?}"), &move |r| {
                r.harness = Harness::Variant { name: name.into() }
            });
        }
        add("variant with all limits", &|r| {
            r.harness = Harness::Variant {
                name: "clone-v1".into(),
            };
            r.model = Some("m".into());
            r.max_cost_usd = Some(1.0);
            r.parallel = Some(2);
        });
        let customs: Vec<(String, String)> = vec![
            ("my/graph.json".into(), "my/runtime.json".into()),
            ("./my/graph.json".into(), " my/runtime.json ".into()),
            (abs_g.clone(), "my/runtime.json".into()),
            ("".into(), "my/runtime.json".into()),
            ("my/graph.json".into(), "  ".into()),
            ("nope/graph.json".into(), "my/runtime.json".into()),
            ("my/graph.json".into(), "nope/runtime.json".into()),
            ("../g.json".into(), "my/runtime.json".into()),
            ("my/../my/graph.json".into(), "my/runtime.json".into()),
            (outside_g.clone(), "my/runtime.json".into()),
            ("my/dir.json".into(), "my/runtime.json".into()),
        ];
        for (g, rt) in customs {
            add(&format!("custom {g:?} {rt:?}"), &move |r| {
                r.harness = Harness::Custom {
                    graph: g.clone(),
                    runtime: rt.clone(),
                }
            });
        }
        let mut start = Vec::new();
        for (name, r) in &cases {
            let out = match start_batch_args(r, root) {
                Ok(argv) => json!({"case": name, "argv": argv}),
                Err(e) => json!({"case": name, "error": e}),
            };
            start.push(out);
        }

        let mut sweep = Vec::new();
        let id_sets: [&[&str]; 3] = [&[], &["sw-1"], &["sw-1", "sw-2"]];
        for ids in id_sets {
            let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
            for label in [None, Some(""), Some("L x")] {
                for task in [None, Some(""), Some("gron")] {
                    for budget in [
                        None,
                        Some(0.0),
                        Some(4.0),
                        Some(2.5),
                        Some(0.30000000000000004),
                    ] {
                        for write in [None, Some(""), Some("out.json")] {
                            let argv = sweep_report_args(
                                &ids,
                                &SweepReportOptions {
                                    label,
                                    task,
                                    budget,
                                    write_runtime: write,
                                },
                            );
                            sweep.push(json!({"ids": ids, "label": label, "task": task, "budget": budget, "writeRuntime": write, "argv": argv}));
                        }
                    }
                }
            }
        }
        let mut value = json!({"start": start, "sweep": sweep});
        let canon = root.canonicalize().unwrap().display().to_string();
        let raw = root.display().to_string();
        let outside_s = outside.path().canonicalize().unwrap().display().to_string();
        let text = value
            .to_string()
            .replace(&canon, "<pb_root>")
            .replace(&raw, "<pb_root>")
            .replace(&outside_s, "<outside>");
        value = serde_json::from_str(&text).unwrap();
        value
    }

    #[test]
    fn every_pb_command_line_matches_the_recorded_argv() {
        let dir = crate::goldens::golden::golden_dir(env!("CARGO_MANIFEST_DIR"));
        crate::goldens::golden::check_json_in(&dir, "pb_exec/argv", &argv_matrix());
    }
}
