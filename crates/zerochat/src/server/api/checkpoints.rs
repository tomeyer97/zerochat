use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::Json;
use serde_json::{Value, json};

use crate::actions::cli::timeout;
use crate::server::args::{cli_args, opt, valid_id, valid_run_id};
use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

/// Checkpoints asked for per `zeroshot checkpoints` call.
const PAGE_SIZE: &str = "100";
/// Pages read at most, so at most 2000 checkpoints: a run with more lists only its first 2000
/// (the answer does not say it was cut).
const MAX_CHECKPOINT_PAGES: usize = 20;

/// The run's checkpoints, read page by page (`--after` the previous page's `nextAfter`).
pub(crate) async fn checkpoints(
    State(state): State<Arc<AppState>>,
    Path((target_id, run_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let target = state.target(&target_id)?;
    valid_run_id(&run_id)?;
    let mut items = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_CHECKPOINT_PAGES {
        let mut options = vec![opt("--limit", PAGE_SIZE)];
        if let Some(a) = &after {
            options.push(opt("--after", valid_id(a, "checkpoint cursor")?));
        }
        let args = cli_args(
            &["checkpoints"],
            options,
            target.cli_name.as_deref(),
            &[&run_id],
        );
        let page = state
            .cli
            .json(&args, timeout::CHECKPOINTS_PAGE)
            .await
            .map_err(ApiError::from_cli)?;
        items.extend(page["checkpoints"].as_array().cloned().unwrap_or_default());
        match page["nextAfter"].as_str() {
            Some(next) => after = Some(next.to_owned()),
            None => break,
        }
    }
    Ok(Json(json!({"checkpoints": items})))
}
