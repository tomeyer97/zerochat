//! zerochat's HTTP API and the web UI it serves.
//!
//! The route table below is the map: one module per family of handlers in `api/`. Around it:
//! - `boundary`: which hosts and requests are answered at all, and the security headers;
//! - `state`: what every handler shares (`AppState`: paths, the zeroshot CLI, the caches);
//! - `run_store`: runs read incrementally and projected into the run view;
//! - `listings`: the run list's rows, rebuilt only for runs that changed;
//! - `benchmarks_state`, `cost`: pb query caching, and whose cost a benchmark run shows;
//! - `error`, `args`: the error answer, and ids checked before they reach the CLI.
//!
//! Two companion tools have pages of their own: pb (the ProgramBench runner, "Benchmarks",
//! configured with `pb_root`) and zsf (Zeroshot features, the slice scheduler behind
//! "Features"). Both are optional; their endpoints say so when the tool is not configured.

mod api;
mod args;
mod benchmarks_state;
mod boundary;
mod cost;
mod error;
mod listings;
mod run_store;
mod state;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};

use api::{
    benchmarks, checkpoints, features, overview, run_actions, run_files, runs, setup, transcripts,
};
use boundary::boundary;

// The goldens call it as `server::valid_run_id`.
pub use api::setup::check as setup_check;
#[cfg(test)]
pub use args::valid_run_id;
pub use boundary::HostAllowlist;
// The web UI (`assets`) answers with the same CSP and API error shape as the routes.
pub(crate) use boundary::INDEX_CSP;
pub(crate) use error::ApiError;
pub use state::AppState;

/// Every route of the API. `boundary` (the last layer, so the first to run) screens each request
/// before compression and the handlers see it.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        // The run list and the top nav.
        .route("/api/overview", get(overview::overview))
        // Can this machine work with zerochat: Zeroshot, the state dir, zsf, pb (first-run card).
        .route("/api/setup", get(setup::setup))
        // Benchmarks (pb).
        .route(
            "/api/benchmarks/batches",
            get(benchmarks::benchmarks_batches).post(benchmarks::benchmarks_start_batch),
        )
        .route(
            "/api/benchmarks/batches/{id}",
            get(benchmarks::benchmarks_batch),
        )
        .route(
            "/api/benchmarks/batches/{id}/stop",
            post(benchmarks::benchmarks_stop_batch),
        )
        .route(
            "/api/benchmarks/compare",
            get(benchmarks::benchmarks_compare),
        )
        .route(
            "/api/benchmarks/start-options",
            get(benchmarks::benchmarks_start_options),
        )
        .route("/api/benchmarks/slots", get(benchmarks::benchmarks_slots))
        .route(
            "/api/benchmarks/harness-preview",
            get(benchmarks::benchmarks_harness_preview),
        )
        .route("/api/benchmarks/sweeps", get(benchmarks::benchmarks_sweeps))
        .route(
            "/api/benchmarks/sweep-report",
            get(benchmarks::benchmarks_sweep_report),
        )
        .route(
            "/api/benchmarks/sweep-report/write-runtime",
            post(benchmarks::benchmarks_write_runtime),
        )
        .route(
            "/api/benchmarks/runs/{run}/score",
            get(benchmarks::benchmarks_run_score),
        )
        // Features (zsf).
        .route("/api/features", get(features::features_list))
        .route("/api/features/{name}", get(features::feature_detail))
        .route("/api/features/{name}/events", get(features::feature_events))
        .route(
            "/api/features/{name}/actions",
            post(features::feature_action),
        )
        .route(
            "/api/features/{name}/slices/{slice}/actions",
            post(features::slice_action),
        )
        .route(
            "/api/features/{name}/slices/{slice}/task/{attempt}",
            get(features::slice_task),
        )
        .route(
            "/api/features/{name}/slices/{slice}/commit",
            get(features::slice_commit),
        )
        .route(
            "/api/features/{name}/slices/{slice}/worktree",
            get(features::slice_worktree),
        )
        .route(
            "/api/features/{name}/changes",
            get(features::feature_changes),
        )
        .route(
            "/api/features/{name}/slices/{slice}/artifacts/{*file}",
            get(features::slice_artifact),
        )
        // One run: its view, live changes, transcripts and files, and the actions on it.
        .route("/api/runs/{target}/{run}", get(runs::run_view))
        .route("/api/runs/{target}/{run}/events", get(runs::run_events))
        .route(
            "/api/runs/{target}/{run}/transcript/{execution}",
            get(transcripts::run_transcript),
        )
        .route(
            "/api/runs/{target}/{run}/checkpoints",
            get(checkpoints::checkpoints),
        )
        .route("/api/runs/{target}/{run}/changes", get(run_files::changes))
        .route("/api/runs/{target}/{run}/diff", get(run_files::diff))
        .route("/api/runs/{target}/{run}/file", get(run_files::file))
        .route("/api/runs/{target}/{run}/tree", get(run_files::tree))
        .route("/api/runs/{target}/{run}/steer", post(run_actions::steer))
        .route("/api/runs/{target}/{run}/stop", post(run_actions::stop))
        .route("/api/runs/{target}/{run}/resume", post(run_actions::resume))
        .route("/api/runs/{target}/{run}/replay", post(run_actions::replay))
        // Everything else is the web UI.
        .fallback(crate::assets::serve)
        // Every poll (overview, run, transcript) is plain JSON; compress it. Excluded from
        // text/event-stream so the live `changed` SSE push isn't held back in a compression
        // buffer.
        .layer(CompressionLayer::new().compress_when(
            DefaultPredicate::new().and(NotForContentType::const_new("text/event-stream")),
        ))
        .layer(middleware::from_fn_with_state(state.clone(), boundary))
        .with_state(state)
}
