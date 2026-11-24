//! `GET /api/overview`: every target with its runs, and what the top nav needs.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clock::now_ms;
use crate::discovery::{Target, TargetKind};
use crate::features;

use crate::server::listings::{ListingRow, local_rows, remote_rows};
use crate::server::state::AppState;

#[derive(Deserialize)]
pub(crate) struct OverviewQuery {
    pub(crate) view: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Overview {
    targets: Vec<TargetOverview>,
    /// Why some targets or sources could not be read, if any.
    issue: Option<String>,
    zeroshot: ZeroshotProgram,
    state_root: String,
    /// zsf is installed, or has a feature.
    features: bool,
    /// Features with a slice that waits for a person.
    features_need_you: usize,
    /// `nav` or `full`.
    view: &'static str,
    now: u64,
}

#[derive(Serialize)]
struct ZeroshotProgram {
    program: String,
    version: Option<String>,
}

/// One target's row: what it is, whether it could be listed, its runs and what its CLI can do.
#[derive(Serialize)]
struct TargetOverview {
    id: String,
    name: String,
    kind: TargetKind,
    origin: Option<String>,
    /// `ok`, `unreachable` or `unsupported`.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    runs: Vec<ListingRow>,
    /// Only for a target that could be listed (`ok`) in the full view.
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Vec<String>>,
}

impl TargetOverview {
    fn ok(target: Target, runs: Vec<ListingRow>, capabilities: Option<Vec<String>>) -> Self {
        Self {
            id: target.id,
            name: target.name,
            kind: target.kind,
            origin: target.origin,
            status: "ok",
            error: None,
            runs,
            capabilities,
        }
    }

    fn failed(target: Target, status: &'static str, error: String) -> Self {
        Self {
            id: target.id,
            name: target.name,
            kind: target.kind,
            origin: target.origin,
            status,
            error: Some(error),
            runs: Vec::new(),
            capabilities: None,
        }
    }
}

/// `GET /api/overview`: every target with its runs. `?view=nav` leaves the runs out (every
/// target with `runs: []`): what a page without a run list still needs (which targets exist,
/// the Features badge), without listing and projecting every run on the machine every few
/// seconds (the full answer is ~50 kB and costs a projection per changed run).
pub(crate) async fn overview(
    State(state): State<Arc<AppState>>,
    Query(q): Query<OverviewQuery>,
) -> Json<Value> {
    let config = state.config();
    let (targets, issue) = state.all_targets(&config);
    let version = state.version().await;
    let nav_only = q.view.as_deref() == Some("nav");
    let mut rows = Vec::new();
    if nav_only {
        rows.extend(
            targets
                .into_iter()
                .map(|t| TargetOverview::ok(t, Vec::new(), None)),
        );
    } else {
        let tasks: Vec<_> = targets
            .into_iter()
            .map(|target| tokio::spawn(target_overview(state.clone(), target)))
            .collect();
        for task in tasks {
            if let Ok(row) = task.await {
                rows.push(row);
            }
        }
    }
    let zsf_root = features::zsf_root(&config);
    let overview = Overview {
        targets: rows,
        issue,
        zeroshot: ZeroshotProgram {
            program: state.cli.program.display().to_string(),
            version,
        },
        state_root: state.state_root.display().to_string(),
        features: features::zsf_bin(&config).is_some() || !features::names(&zsf_root).is_empty(),
        features_need_you: features::needs_you_count(&zsf_root),
        view: if nav_only { "nav" } else { "full" },
        now: now_ms(),
    };
    Json(serde_json::to_value(overview).expect("the overview serialises"))
}

async fn target_overview(state: Arc<AppState>, target: Target) -> TargetOverview {
    match target.kind {
        TargetKind::Local => {
            let runs = local_rows(&state, &target, state.state_root.clone()).await;
            let capabilities = state.capabilities(&target).await;
            TargetOverview::ok(target, runs, Some(capabilities))
        }
        TargetKind::Observe => {
            let mut runs = Vec::new();
            for root in target.roots.clone() {
                runs.extend(local_rows(&state, &target, root).await);
            }
            let capabilities = state.capabilities(&target).await; // always empty (see `capabilities`)
            TargetOverview::ok(target, runs, Some(capabilities))
        }
        TargetKind::Direct => {
            let origin = target.origin.clone().unwrap_or_default();
            match state.remote.listings(&origin).await {
                Ok(listings) => {
                    let runs = remote_rows(&state, &target, &origin, listings).await;
                    let capabilities = state.capabilities(&target).await;
                    TargetOverview::ok(target, runs, Some(capabilities))
                }
                Err(error) => TargetOverview::failed(target, "unreachable", format!("{error:#}")),
            }
        }
        TargetKind::Hosted => TargetOverview::failed(
            target,
            "unsupported",
            "Hosted (Platform) targets are not supported by zerochat yet.".to_owned(),
        ),
    }
}
