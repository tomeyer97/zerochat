//! `/api/benchmarks/*`: the Benchmarks page. pb (the ProgramBench runner) keeps its batches and
//! runs in `<pb_root>/pb.sqlite`; zerochat reads that database directly and runs the `pb` program
//! for what needs pb's own code (starting and stopping a batch, slots, sweep reports).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use axum::extract::{Path, Query, State};
use axum::response::Json;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::benchmarks;
use crate::pb_exec;

use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

/// The error code every endpoint here answers (404) when pb is not configured: not a failure,
/// the web shows its neutral "Benchmarks needs pb" page for it.
pub(crate) const PB_NOT_CONFIGURED: &str = "pb_not_configured";

/// Where pb is, from `config.toml`: every endpoint here needs it.
struct PbSource {
    root: PathBuf,
    /// The `pb` invocation (explicit `pb_command`, else `uv run --project <pb_root> pb`).
    command: String,
}

impl PbSource {
    fn of(state: &AppState) -> ApiResult<Self> {
        let config = state.config();
        let no_pb_source = || {
            ApiError::not_found(
                PB_NOT_CONFIGURED,
                "Benchmarks needs pb (ProgramBench runner), which is not configured here: set `pb_root` in zerochat's config.toml.",
            )
        };
        let root = config.pb_root.clone().ok_or_else(no_pb_source)?;
        let command = config.pb_command().ok_or_else(no_pb_source)?;
        Ok(Self { root, command })
    }
}

fn to_json(value: impl serde::Serialize) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(value).map_err(|e| ApiError::internal(e.into()))?,
    ))
}

// ---- batches ----

/// `GET /api/benchmarks/batches`: every batch (real `batches` rows plus label pseudo-batches),
/// newest first.
pub(crate) async fn benchmarks_batches(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let batches = state
        .cached_json("batches".to_owned(), &pb.root, |root| {
            Ok(serde_json::to_value(benchmarks::list_batches(root)?)?)
        })
        .await?;
    Ok(Json(json!({"batches": batches})))
}

/// `GET /api/benchmarks/batches/{id}`: one batch's task × seed matrix.
pub(crate) async fn benchmarks_batch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let batch_id = id.clone();
    let mut detail = state
        .cached_json(format!("batch:{id}"), &pb.root, move |root| {
            Ok(serde_json::to_value(benchmarks::batch_detail(
                root, &batch_id,
            )?)?)
        })
        .await?;
    if detail.is_null() {
        // An id `pb run --detach` printed moments ago has no `batches` row until pb's runner
        // writes it; say "starting" (or why it failed to), not "not found".
        let started = state.benchmarks.started(&id);
        return match benchmarks::pending_batch(&pb.root, &id, started, SystemTime::now()) {
            Some(pending) => to_json(pending),
            None => Err(ApiError::not_found(
                "batch_not_found",
                format!("No batch `{id}`."),
            )),
        };
    }
    benchmarks::fill_live_cells(&pb.root, &mut detail);
    Ok(Json(detail))
}

/// `POST /api/benchmarks/batches`: `pb run --detach ...`. Starts even while other batches run
/// (their runs share the host's `max_concurrent_runs` slots); an older pb that still runs one
/// batch at a time refuses, and its message is surfaced verbatim.
pub(crate) async fn benchmarks_start_batch(
    State(state): State<Arc<AppState>>,
    Json(body): Json<pb_exec::StartBatchRequest>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let started = pb_exec::start_batch(&pb.root, &pb.command, &body)
        .await
        .map_err(ApiError::from_pb)?;
    state
        .benchmarks
        .remember_started(started.id.clone(), started.pid);
    Ok(Json(json!({"id": started.id})))
}

/// `POST /api/benchmarks/batches/{id}/stop`: `pb stop <id>` (force-stops its Zeroshot runs,
/// marks them interrupted). Refused for a `label:` pseudo-batch: there is no pb process behind
/// pre-existing runs grouped only by label.
pub(crate) async fn benchmarks_stop_batch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(_): Json<Value>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let message = pb_exec::stop_batch(&pb.root, &pb.command, &id)
        .await
        .map_err(ApiError::from_pb)?;
    Ok(Json(json!({"stopped": true, "message": message.trim()})))
}

#[derive(Deserialize)]
pub(crate) struct CompareQuery {
    a: Option<String>,
    b: Option<String>,
}

/// `GET /api/benchmarks/compare?a=<batch>&b=<batch>`: per task (score Δ, seed spread, pb's noise
/// verdict), totals, and per node (mean cost/time Δ) — all computed here, not in the browser.
pub(crate) async fn benchmarks_compare(
    State(state): State<Arc<AppState>>,
    Query(q): Query<CompareQuery>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let (a, b) = match (q.a.filter(|s| !s.is_empty()), q.b.filter(|s| !s.is_empty())) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            return Err(ApiError::bad_request(
                "missing_batch",
                "Pick two batches to compare (?a=...&b=...).",
            ));
        }
    };
    let cache_key = format!("compare:{a}:{b}");
    let compare = state
        .cached_json(cache_key, &pb.root, move |root| {
            Ok(serde_json::to_value(benchmarks::compare_batches(
                root, &a, &b,
            )?)?)
        })
        .await?;
    Ok(Json(compare))
}

/// `GET /api/benchmarks/runs/{run}/score`: one run's score card, found by pb's own run id or by
/// the Zeroshot run id it ran as (what a zerochat run page has).
pub(crate) async fn benchmarks_run_score(
    State(state): State<Arc<AppState>>,
    Path(run): Path<String>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let run_id = run.clone();
    let score = state
        .cached_json(format!("score:{run}"), &pb.root, move |root| {
            Ok(serde_json::to_value(benchmarks::run_score(root, &run_id)?)?)
        })
        .await?;
    if score.is_null() {
        return Err(ApiError::not_found(
            "run_not_found",
            format!("pb has no run `{run}`."),
        ));
    }
    Ok(Json(score))
}

// ---- the start form ----

/// `GET /api/benchmarks/start-options`: task sets, repo tasks, harness variants and pb's own
/// default limits, read straight from `pb.toml` (no need to shell out just to fill in a form).
pub(crate) async fn benchmarks_start_options(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let options = tokio::task::spawn_blocking(move || benchmarks::harness_options(&pb.root))
        .await
        .map_err(|e| ApiError::internal(e.into()))?
        .map_err(ApiError::internal)?;
    to_json(options)
}

/// `GET /api/benchmarks/slots`: `pb slots --json` — how many runs hold one of the host's run
/// slots and how many wait for one, across every batch and pb process on this host. pb runs
/// batches concurrently, so the Start form says this instead of refusing while a batch runs.
pub(crate) async fn benchmarks_slots(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let slots = pb_exec::run_slots(&pb.root, &pb.command)
        .await
        .map_err(ApiError::from_pb)?;
    to_json(slots)
}

#[derive(Deserialize)]
pub(crate) struct HarnessPreviewQuery {
    graph: Option<String>,
    runtime: Option<String>,
    model: Option<String>,
}

/// `GET /api/benchmarks/harness-preview?graph=&runtime=&model=`: what the start form's harness
/// choice would run (its agent nodes with model and effort, which known harness it is) and
/// whether pb could start it at all — before Start, not after. Paths are relative to `pb_root`
/// and must stay inside it.
pub(crate) async fn benchmarks_harness_preview(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HarnessPreviewQuery>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let preview = tokio::task::spawn_blocking(move || {
        crate::harness::preview(
            &pb.root,
            q.graph.as_deref().unwrap_or(""),
            q.runtime.as_deref().unwrap_or(""),
            q.model.as_deref(),
        )
    })
    .await
    .map_err(|e| ApiError::internal(e.into()))?;
    to_json(preview)
}

// ---- sweeps ----

/// `GET /api/benchmarks/sweeps`: `pb sweeps`' list, newest first.
pub(crate) async fn benchmarks_sweeps(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let sweeps = state
        .cached_json("sweeps".to_owned(), &pb.root, |root| {
            Ok(serde_json::to_value(benchmarks::list_sweeps(root)?)?)
        })
        .await?;
    Ok(Json(json!({"sweeps": sweeps})))
}

#[derive(Deserialize)]
pub(crate) struct SweepReportQuery {
    ids: Option<String>,
    label: Option<String>,
    task: Option<String>,
    budget: Option<f64>,
}

fn split_ids(ids: &Option<String>) -> Vec<String> {
    ids.as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `GET /api/benchmarks/sweep-report?ids=<comma>&label=&task=&budget=`: `pb sweep-report
/// --json`, pooling the given sweeps (or every sweep matching `label`/`task` when `ids` is
/// empty) — pb's own allocation math, not re-derived here.
pub(crate) async fn benchmarks_sweep_report(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SweepReportQuery>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    let ids = split_ids(&q.ids);
    let report = pb_exec::sweep_report(
        &pb.root,
        &pb.command,
        &ids,
        q.label.as_deref(),
        q.task.as_deref(),
        q.budget,
        None,
    )
    .await
    .map_err(ApiError::from_pb)?;
    Ok(Json(report))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteRuntimeBody {
    #[serde(default)]
    ids: Vec<String>,
    label: Option<String>,
    task: Option<String>,
    budget: Option<f64>,
    path: String,
}

/// `POST /api/benchmarks/sweep-report/write-runtime`: the same report, plus
/// `--write-runtime <path>` (relative to `pb_root`, resolved by pb itself the same way `pb
/// sweep-report --write-runtime` always has). The confirm dialog is the UI's job; this endpoint
/// just does what it is asked, once.
pub(crate) async fn benchmarks_write_runtime(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WriteRuntimeBody>,
) -> ApiResult<Json<Value>> {
    let pb = PbSource::of(&state)?;
    if body.path.trim().is_empty() {
        return Err(ApiError::bad_request(
            "missing_path",
            "Where should the runtime file be written?",
        ));
    }
    let report = pb_exec::sweep_report(
        &pb.root,
        &pb.command,
        &body.ids,
        body.label.as_deref(),
        body.task.as_deref(),
        body.budget,
        Some(body.path.as_str()),
    )
    .await
    .map_err(ApiError::from_pb)?;
    Ok(Json(report))
}
