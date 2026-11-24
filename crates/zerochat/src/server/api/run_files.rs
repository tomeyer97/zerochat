//! A run's files: what it changed, diffs, a file, the tree. Only for runs whose workspace is on
//! this machine (`workspace::Workspace` says where); the git reads are `actions::git`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::actions::{self, git::ChangedFile, git::InvalidInput};
use crate::discovery::TargetKind;
use crate::source::RunData;
use crate::workspace::{BaseKind, NoSnapshot, ReadError, Workspace, run_base, workspace_of};

use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

#[derive(Deserialize)]
pub(crate) struct RangeQuery {
    from: Option<String>,
    to: Option<String>,
    path: Option<String>,
    commit: Option<String>,
}

/// Why no workspace was found, as the changes view says it (it is the first to ask).
const WORKSPACE_NOT_HERE_OR_UNKNOWN: &str =
    "This run's workspace is not on this machine (or Zeroshot did not record where it is).";
/// The same, as the diff, file and tree views say it.
const WORKSPACE_NOT_HERE: &str = "This run's workspace is not on this machine.";

impl From<NoSnapshot> for ApiError {
    fn from(_: NoSnapshot) -> Self {
        ApiError::unavailable(
            "workspace_unavailable",
            "This run's worktree was removed and Zeroshot recorded no snapshot of it.",
        )
    }
}

/// The run and its workspace on this machine. Refused when the run is on another machine
/// ([`remote_files`]) or its workspace is not here (`missing` says so).
async fn local_workspace(
    state: &AppState,
    target_id: &str,
    run_id: &str,
    missing: &str,
) -> ApiResult<(RunData, Workspace)> {
    let target = state.target(target_id)?;
    let data = state.run(&target, run_id).await?;
    if target.kind != TargetKind::Local {
        return Err(remote_files());
    }
    let ws = workspace_of(&data)
        .ok_or_else(|| ApiError::unavailable("workspace_unavailable", missing))?;
    Ok((data, ws))
}

/// Files of a run on another machine: Zeroshot does not serve a run's workspace, so zerochat
/// shows files only for runs whose workspace is on this machine.
pub(crate) fn remote_files() -> ApiError {
    ApiError::unavailable(
        "not_supported",
        "zerochat shows files only for runs whose workspace is on this machine.",
    )
}

/// The changes view's answer.
#[derive(Serialize)]
struct Changes {
    files: Vec<ChangedFile>,
    /// Where the files were read.
    workspace: String,
    from: String,
    /// None: the working tree.
    to: Option<String>,
    head: Option<String>,
    /// How `from` was chosen.
    base: BaseKind,
    /// What the range shows, for the person reading it ("" when it says nothing more).
    note: &'static str,
}

/// What a range of changes shows, in words. `to_is_now`: the range ends "now" (no `to` asked).
fn changes_note(base: BaseKind, to_is_now: bool, workspace_removed: bool) -> &'static str {
    if to_is_now && workspace_removed {
        "The run's worktree was removed after it was merged; this is its last snapshot."
    } else if base == BaseKind::CheckedOut {
        "Compared with the commit checked out when the run started; the workspace as it is now, so later edits show up too."
    } else if to_is_now {
        "The workspace as it is now; later edits show up too."
    } else {
        ""
    }
}

/// Files changed by the run (`from` = the run's base by default, `to` = working tree by
/// default), or by one execution when its snapshot commits are given.
pub(crate) async fn changes(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
    Query(q): Query<RangeQuery>,
) -> ApiResult<Json<Value>> {
    let (data, ws) =
        local_workspace(&state, &target_id, &run_id, WORKSPACE_NOT_HERE_OR_UNKNOWN).await?;
    let workspace = &ws.path;
    let (base, base_kind) = match q.from.clone() {
        Some(from) => (from, BaseKind::Execution),
        None => run_base(&data, workspace).await.ok_or_else(|| {
            ApiError::unavailable(
                "no_base",
                format!("{} has no commit to compare with.", workspace.display()),
            )
        })?,
    };
    if !actions::rev_exists(workspace, &base).await {
        return Err(ApiError::unavailable(
            "base_missing",
            format!("The commit {base} is no longer in {}.", workspace.display()),
        ));
    }
    let to = ws.now(q.to.clone())?;
    let files = actions::changed_files(workspace, &base, to.as_deref())
        .await
        .map_err(ApiError::internal)?;
    let head = if ws.removed {
        to.clone()
    } else {
        actions::head(workspace).await
    };
    let changes = Changes {
        files,
        workspace: workspace.display().to_string(),
        from: base,
        to,
        head,
        base: base_kind,
        note: changes_note(base_kind, q.to.is_none(), ws.removed),
    };
    // Through `Value`, so the keys come out sorted like every other answer.
    Ok(Json(json!(changes)))
}

pub(crate) async fn diff(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
    Query(q): Query<RangeQuery>,
) -> ApiResult<Json<Value>> {
    let (data, ws) = local_workspace(&state, &target_id, &run_id, WORKSPACE_NOT_HERE).await?;
    let base = match q.from.clone() {
        Some(from) => from,
        None => run_base(&data, &ws.path)
            .await
            .map(|(b, _)| b)
            .unwrap_or_else(|| "HEAD".into()),
    };
    let to = ws.now(q.to.clone())?;
    let text = actions::diff(&ws.path, &base, to.as_deref(), q.path.as_deref())
        .await
        .map_err(file_error)?;
    Ok(Json(json!({"diff": text})))
}

/// Refused paths and revisions, and workspace read refusals, as client errors; anything else
/// (git failing) as a server error.
pub(crate) fn file_error(error: anyhow::Error) -> ApiError {
    if let Some(invalid) = error.downcast_ref::<InvalidInput>() {
        return ApiError::bad_request("invalid_path", invalid.to_string());
    }
    let Some(read) = error.downcast_ref::<ReadError>() else {
        return ApiError::internal(error);
    };
    let status = match read {
        ReadError::InvalidPath => StatusCode::BAD_REQUEST,
        ReadError::NotFound => StatusCode::NOT_FOUND,
        ReadError::Outside => StatusCode::FORBIDDEN,
        ReadError::NotRegular(_) => StatusCode::UNPROCESSABLE_ENTITY,
        ReadError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        ReadError::TimedOut => StatusCode::GATEWAY_TIMEOUT,
        ReadError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, "file_unavailable", read.to_string())
}

pub(crate) async fn file(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
    Query(q): Query<RangeQuery>,
) -> ApiResult<Json<Value>> {
    let (_, ws) = local_workspace(&state, &target_id, &run_id, WORKSPACE_NOT_HERE).await?;
    let path = q
        .path
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("path_required", "Which file?"))?;
    let commit = ws.now(q.commit.clone())?;
    let content = actions::file(&ws.path, commit.as_deref(), path)
        .await
        .map_err(file_error)?;
    Ok(Json(
        json!({"path": path, "commit": commit, "content": content}),
    ))
}

pub(crate) async fn tree(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
    Query(q): Query<RangeQuery>,
) -> ApiResult<Json<Value>> {
    let (_, ws) = local_workspace(&state, &target_id, &run_id, WORKSPACE_NOT_HERE).await?;
    let commit = ws.now(q.commit.clone())?;
    let paths = actions::tree(&ws.path, commit.as_deref())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({"paths": paths, "commit": commit})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_changes_note_follows_the_base_and_the_range() {
        let removed =
            "The run's worktree was removed after it was merged; this is its last snapshot.";
        let checked_out = "Compared with the commit checked out when the run started; the workspace as it is now, so later edits show up too.";
        let now = "The workspace as it is now; later edits show up too.";
        assert_eq!(changes_note(BaseKind::CheckedOut, true, true), removed);
        assert_eq!(changes_note(BaseKind::CheckedOut, false, true), checked_out);
        assert_eq!(changes_note(BaseKind::CheckedOut, true, false), checked_out);
        assert_eq!(changes_note(BaseKind::Source, true, false), now);
        assert_eq!(changes_note(BaseKind::Snapshot, false, false), "");
        assert_eq!(changes_note(BaseKind::Execution, false, true), "");
    }

    #[test]
    fn refused_paths_and_revisions_are_bad_requests() {
        for (input, message) in [
            (InvalidInput::Path, "invalid path"),
            (InvalidInput::Revision, "invalid revision"),
        ] {
            let api = file_error(anyhow::anyhow!(input));
            assert_eq!(
                (api.status, api.code.as_str(), api.message.as_str()),
                (StatusCode::BAD_REQUEST, "invalid_path", message)
            );
        }
        assert_eq!(
            file_error(anyhow::anyhow!(ReadError::Outside)).status,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            file_error(anyhow::anyhow!("git diff: fatal")).status,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
