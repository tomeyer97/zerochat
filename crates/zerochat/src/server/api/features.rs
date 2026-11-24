//! `/api/features/*`: the Features page. zsf (Zeroshot features) is the scheduler that runs a
//! feature's slices as Zeroshot runs; zerochat reads its state root and runs the `zsf` program
//! for actions.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::clock::now_ms;
use crate::features;

use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

/// `GET /api/features/{name}/events` answers at most this many events (the newest).
const FEATURE_EVENTS_LIMIT: usize = 2000;

fn no_feature(name: &str) -> ApiError {
    ApiError::not_found("feature_not_found", format!("No feature `{name}`."))
}

/// Runs a features read on a blocking thread with zsf's root, the run roots and the run cache.
async fn with_features<T, F>(state: &Arc<AppState>, read: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&features::Ctx) -> anyhow::Result<T> + Send + 'static,
{
    let config = state.config();
    let (root, bin) = (features::zsf_root(&config), features::zsf_bin(&config));
    let roots = state.run_roots(&config);
    let runs = state.run_facts.clone();
    tokio::task::spawn_blocking(move || {
        let ctx = features::Ctx {
            root: &root,
            roots: &roots,
            runs: &runs,
            zsf_bin: bin.as_deref(),
            now: now_ms(),
        };
        read(&ctx)
    })
    .await
    .map_err(|e| ApiError::internal(e.into()))?
    .map_err(ApiError::internal)
}

/// [`with_features`] for a read that may find nothing: `missing` when it does.
async fn find<T, F>(state: &Arc<AppState>, missing: ApiError, read: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&features::Ctx) -> anyhow::Result<Option<T>> + Send + 'static,
{
    with_features(state, read).await?.ok_or(missing)
}

/// `GET /api/features`: one row per feature.
pub(crate) async fn features_list(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let rows = with_features(&state, |ctx| Ok(features::list(ctx))).await?;
    Ok(Json(json!({"features": rows, "now": now_ms()})))
}

/// `GET /api/features/{name}`: plan, state and per-slice detail with the actions zsf accepts now.
pub(crate) async fn feature_detail(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<Value>> {
    let detail = find(&state, no_feature(&name), move |ctx| {
        features::detail(ctx, &name)
    })
    .await?;
    let mut value = serde_json::to_value(detail).map_err(|e| ApiError::internal(e.into()))?;
    value["now"] = json!(now_ms());
    Ok(Json(value))
}

#[derive(Deserialize)]
pub(crate) struct EventsQuery {
    pub(crate) slice: Option<String>,
}

/// `GET /api/features/{name}/events?slice=`: structured events, oldest first (the newest
/// `FEATURE_EVENTS_LIMIT`).
pub(crate) async fn feature_events(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(query): Query<EventsQuery>,
) -> ApiResult<Json<Value>> {
    let slice = query.slice.filter(|s| !s.is_empty());
    let events = find(&state, no_feature(&name), move |ctx| {
        features::events(
            ctx.root,
            ctx.roots,
            &name,
            slice.as_deref(),
            FEATURE_EVENTS_LIMIT,
        )
    })
    .await?;
    Ok(Json(json!({"events": events})))
}

#[derive(Deserialize)]
pub(crate) struct ChangesQuery {
    diff: Option<bool>,
}

/// `GET /api/features/{name}/changes?diff=true`: the feature branch against its base.
pub(crate) async fn feature_changes(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(query): Query<ChangesQuery>,
) -> ApiResult<Json<Value>> {
    let with_diff = query.diff.unwrap_or(false);
    let changes = find(&state, no_feature(&name), move |ctx| {
        features::changes(ctx.root, &name, with_diff)
    })
    .await?;
    Ok(Json(json!(changes)))
}

/// `GET /api/features/{name}/slices/{slice}/task/{attempt}`: the exact task, and its diff to the
/// previous attempt's.
pub(crate) async fn slice_task(
    State(state): State<Arc<AppState>>,
    Path((name, slice, attempt)): Path<(String, String, u32)>,
) -> ApiResult<Json<Value>> {
    let missing = ApiError::not_found("task_not_found", "This attempt has no task file.");
    let task = find(&state, missing, move |ctx| {
        features::task(ctx.root, &name, &slice, attempt)
    })
    .await?;
    Ok(Json(json!(task)))
}

/// `GET /api/features/{name}/slices/{slice}/commit`: the merged squash commit and its diff.
pub(crate) async fn slice_commit(
    State(state): State<Arc<AppState>>,
    Path((name, slice)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let missing = ApiError::not_found("commit_not_found", "This slice has no merged commit.");
    let commit = find(&state, missing, move |ctx| {
        features::commit(ctx.root, &name, &slice)
    })
    .await?;
    Ok(Json(json!(commit)))
}

/// `GET /api/features/{name}/slices/{slice}/worktree`: a stopped slice's worktree against the
/// commit it forked from (what Accept merges).
pub(crate) async fn slice_worktree(
    State(state): State<Arc<AppState>>,
    Path((name, slice)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let missing = ApiError::not_found("worktree_not_found", "This slice has no worktree.");
    let diff = find(&state, missing, move |ctx| {
        features::worktree_diff(ctx.root, &name, &slice)
    })
    .await?;
    Ok(Json(json!(diff)))
}

/// `GET /api/features/{name}/slices/{slice}/artifacts/{file}`: a saved artifact's text.
pub(crate) async fn slice_artifact(
    State(state): State<Arc<AppState>>,
    Path((name, slice, file)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let missing = ApiError::not_found("artifact_not_found", "No such artifact.");
    let path = file.clone();
    let content = find(&state, missing, move |ctx| {
        features::artifact(ctx.root, &name, &slice, &file)
    })
    .await?;
    Ok(Json(json!({"path": path, "content": content})))
}

/// `POST /api/features/{name}/actions` `{action: start|stop|unpause|max_parallel, maxParallel?}`.
pub(crate) async fn feature_action(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<features::ActionBody>,
) -> ApiResult<Json<Value>> {
    feature_act(state, name, None, body).await
}

/// `POST /api/features/{name}/slices/{slice}/actions`
/// `{action: approve|retry|accept|skip|edit, notes?, fresh?, title?, deps?, spec?}`.
pub(crate) async fn slice_action(
    State(state): State<Arc<AppState>>,
    Path((name, slice)): Path<(String, String)>,
    Json(body): Json<features::ActionBody>,
) -> ApiResult<Json<Value>> {
    feature_act(state, name, Some(slice), body).await
}

async fn feature_act(
    state: Arc<AppState>,
    name: String,
    slice: Option<String>,
    body: features::ActionBody,
) -> ApiResult<Json<Value>> {
    let config = state.config();
    let (root, bin) = (features::zsf_root(&config), features::zsf_bin(&config));
    let outcome = tokio::task::spawn_blocking(move || {
        features::act(&root, bin.as_deref(), &name, slice.as_deref(), body)
    })
    .await
    .map_err(|e| ApiError::internal(e.into()))?;
    match outcome {
        Ok(outcome) => Ok(Json(json!(outcome))),
        Err(features::ActError::NotFound(message)) => {
            Err(ApiError::not_found("not_found", message))
        }
        Err(features::ActError::Refused(message)) => {
            Err(ApiError::new(StatusCode::CONFLICT, "refused", message))
        }
        Err(features::ActError::Bad(message)) => {
            Err(ApiError::bad_request("invalid_request", message))
        }
        Err(features::ActError::Failed(error)) => Err(ApiError::internal(error)),
    }
}
