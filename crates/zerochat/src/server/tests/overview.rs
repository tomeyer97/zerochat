//! The overview's nav-only view.

use axum::extract::{Query, State};
use axum::response::Json;
use serde_json::json;

use crate::server::api::overview::{OverviewQuery, overview};

use super::pb_state;

/// The nav-only overview (Benchmarks/Features pages poll it instead of the full run list)
/// still names every target — the `pb` target is what turns the Benchmarks nav on — but
/// lists no runs.
#[tokio::test]
async fn overview_nav_view_lists_targets_without_runs() {
    let dir = tempfile::tempdir().unwrap();
    let state = pb_state(dir.path());
    let Json(nav) = overview(
        State(state.clone()),
        Query(OverviewQuery {
            view: Some("nav".to_owned()),
        }),
    )
    .await;
    assert_eq!(nav["view"], "nav");
    let targets = nav["targets"].as_array().unwrap();
    assert!(targets.iter().any(|t| t["id"] == "pb"), "{nav}");
    assert!(targets.iter().all(|t| t["runs"] == json!([])), "{nav}");
    let Json(full) = overview(State(state), Query(OverviewQuery { view: None })).await;
    assert_eq!(full["view"], "full");
}
