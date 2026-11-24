//! Run-list rows (the overview's `runs`): each run's listing plus a summary computed by the same
//! projection as the run page, cached per run and rebuilt only when the run changed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::stream::StreamExt;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::clock::now_ms;
use crate::context::{self, RunContext, RunContextProvider};
use crate::discovery::Target;
use crate::projection::{self, RunView};
use crate::source::{self, RunData, RunListing};

use super::cost::{ZeroshotCost, pb_cost};
use super::run_store::{controller_on_this_host, merge_events, trust_controller_check};
use super::state::{AppState, context_providers};

/// A local root lists at most this many runs.
const MAX_LISTED_RUNS: usize = 300;
/// How many of a direct target's runs are read again at the same time.
const REMOTE_READS_AT_ONCE: usize = 6;

#[derive(Default)]
pub(crate) struct Listings {
    /// Rows by `(target id, run id)`, valid while the run's `ListingStamp` is unchanged.
    rows: Mutex<HashMap<(String, String), CachedListing>>,
    /// Direct targets whose rows are being refreshed in the background.
    refreshing: std::sync::Mutex<HashSet<String>>,
}

struct CachedListing {
    stamp: ListingStamp,
    data: RunData,
    row: ListingRow,
}

/// What a cached row was built from: while it is unchanged, so is the row.
#[derive(Clone, Debug, PartialEq)]
enum ListingStamp {
    /// A local run: its ledger files (`source::ledger_stamp`), whether its controller is alive,
    /// and its successor.
    Local {
        ledger: Vec<(u64, u128)>,
        controller_alive: bool,
        resumed_as: Option<String>,
    },
    /// A direct target's run: its history cursor and phase, from the target's listing.
    Remote {
        cursor: Option<String>,
        phase: String,
    },
    /// A row built without reading the run (it could not be read): matches no stamp.
    Unread,
}

impl ListingStamp {
    fn remote(listing: &RunListing) -> Self {
        Self::Remote {
            cursor: listing.cursor.clone(),
            phase: listing.phase.clone(),
        }
    }
}

/// One run-list row: the listing, its summary, and its context (pb/zsf) when a provider
/// recognises the run, for the sidebar's task-name label.
#[derive(Clone, Serialize)]
pub(crate) struct ListingRow {
    #[serde(flatten)]
    listing: RunListing,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<RunSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<RunContext>,
}

impl ListingRow {
    fn new(listing: RunListing, view: Option<&RunView>, context: Option<RunContext>) -> Self {
        let mut summary = view.map(RunSummary::of);
        if let (Some(summary), Some(context)) = (&mut summary, &context) {
            summary.apply_benchmark_cost(context);
        }
        Self {
            listing,
            summary,
            context,
        }
    }
}

/// A run's status, cause, duration, cost and lineage, from the same projection as the run page,
/// so the list and the page agree.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunSummary {
    /// `None` while a direct target's row is stale: the listing's own phase shows instead.
    status: Option<String>,
    cause: Option<String>,
    failure_class: Option<&'static str>,
    failure_node: Option<String>,
    started_at: Option<u64>,
    ended_at: Option<u64>,
    duration_ms: Option<u64>,
    cost_usd: Option<f64>,
    cost_estimated: bool,
    resumed_as: Option<String>,
    resumed_from: Option<String>,
    replay_of: Option<serde_json::Value>,
    #[serde(flatten)]
    zeroshot_cost: Option<ZeroshotCost>,
}

impl RunSummary {
    fn of(view: &RunView) -> Self {
        let failure = view.failure.as_ref();
        Self {
            status: Some(view.status.clone()),
            cause: failure.map(|f| f.cause.clone()),
            failure_class: failure.map(|f| f.class),
            failure_node: failure.and_then(|f| f.node.clone()),
            started_at: view.started_at,
            ended_at: view.ended_at,
            duration_ms: view
                .started_at
                .zip(view.ended_at.or(view.last_activity_at))
                .map(|(s, e)| e.saturating_sub(s)),
            cost_usd: view.cost_usd,
            cost_estimated: view.cost_estimated,
            resumed_as: view.resumed_as.clone(),
            resumed_from: view.resumed_from.clone(),
            replay_of: view
                .replay_of
                .as_ref()
                .and_then(|r| r.get("runId"))
                .cloned(),
            zeroshot_cost: None,
        }
    }

    /// A benchmark run shows pb's cost; Zeroshot's is kept aside (`cost` module).
    fn apply_benchmark_cost(&mut self, context: &RunContext) {
        let Some(pb_cost) = pb_cost(context) else {
            return;
        };
        self.zeroshot_cost = Some(ZeroshotCost {
            zeroshot_cost_usd: self.cost_usd,
            zeroshot_cost_estimated: self.cost_estimated,
            cost_source: "pb",
        });
        self.cost_usd = Some(pb_cost);
        self.cost_estimated = false;
    }

    /// Stale until the background refresh lands: the listing's own status wins.
    fn mark_stale(&mut self) {
        self.status = None;
        self.cause = None;
    }
}

/// Rows for one local-shaped root: the implicit `$ZEROSHOT_STATE_DIR` for `TargetKind::Local`,
/// or one of a configured source's roots for `TargetKind::Observe` (called once per root;
/// a source's rows are the concatenation). A run whose ledger files, controller and successor
/// are unchanged is not read again; a changed one is read incrementally. A run that cannot be
/// read right now (for example mid-write) keeps its last good row.
pub(crate) async fn local_rows(
    state: &Arc<AppState>,
    target: &Target,
    root: PathBuf,
) -> Vec<ListingRow> {
    let ids: Vec<String> = source::local_run_ids(&root)
        .into_iter()
        .take(MAX_LISTED_RUNS)
        .collect();
    let mut cache = state.listings.rows.lock().await;
    let mut previous: HashMap<String, CachedListing> = ids
        .iter()
        .filter_map(|id| Some((id.clone(), cache.remove(&(target.id.clone(), id.clone()))?)))
        .collect();
    let config = state.config();
    let pb_root = if controller_on_this_host(target) {
        None
    } else {
        config.pb_root.clone()
    };
    let providers = context_providers(&config);
    let target_for_reads = target.clone();
    let state_for_reads = state.clone();
    let computed = tokio::task::spawn_blocking(move || {
        // One pb run per pb root: one small pb.sqlite lookup per root, not per run.
        let controller_here = trust_controller_check(&target_for_reads, pb_root.as_deref(), &root);
        let reads = LocalReads {
            state: &state_for_reads,
            target: &target_for_reads,
            root: &root,
            providers: &providers,
            controller_here,
        };
        ids.into_iter()
            .map(|id| (reads.refresh(&id, previous.remove(&id)), id))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let mut rows = Vec::with_capacity(computed.len());
    for (entry, id) in computed {
        rows.push(entry.row.clone());
        cache.insert((target.id.clone(), id), entry);
    }
    rows
}

/// What every run of one local root is read with.
struct LocalReads<'a> {
    state: &'a AppState,
    target: &'a Target,
    root: &'a Path,
    providers: &'a [Box<dyn RunContextProvider>],
    controller_here: bool,
}

impl LocalReads<'_> {
    /// `id`'s row: the cached one while its stamp holds, else read again from the cached cursor.
    fn refresh(&self, id: &str, cached: Option<CachedListing>) -> CachedListing {
        let mut facts = source::local_facts(self.root, id);
        if !self.controller_here {
            facts.controller_elsewhere();
        }
        let stamp = ListingStamp::Local {
            ledger: source::ledger_stamp(self.root, id),
            controller_alive: facts.controller_liveness.is_alive(),
            resumed_as: facts.resumed_as.clone(),
        };
        let cached = match cached {
            Some(c) if c.stamp == stamp => return c,
            other => other,
        };
        let mut data = cached
            .as_ref()
            .map(|c| c.data.clone())
            .unwrap_or_else(|| RunData {
                run_id: id.to_owned(),
                ..Default::default()
            });
        let after = data.events.last().map(|e| e.seq).unwrap_or(0);
        let context = context::lookup(self.providers, id);
        match source::read_local(self.root, id, after) {
            Ok((stored, events)) => {
                merge_events(&mut data, stored, events, after);
                if facts.controller_liveness.is_alive() || data.snapshot["phase"] != "running" {
                    facts.agent_processes.clear();
                }
                data.local = Some(facts);
                let listing = source::local_listing_from(id, &data);
                // The run page's projection, with the usage live from the provider's session log,
                // so an interrupted run's tokens and cost show here too.
                let view = self.state.project(self.target, data.clone());
                let row = ListingRow::new(listing, Some(&view), context);
                CachedListing { stamp, data, row }
            }
            Err(_) => cached.unwrap_or_else(|| {
                let row = ListingRow::new(source::local_listing(self.root, id), None, context);
                CachedListing {
                    stamp: ListingStamp::Unread,
                    data,
                    row,
                }
            }),
        }
    }
}

/// Direct-target rows: what is cached now; runs whose history cursor moved are re-read in the
/// background (a few at a time, incrementally), so a slow target never holds up the list.
pub(crate) async fn remote_rows(
    state: &Arc<AppState>,
    target: &Target,
    origin: &str,
    listings: Vec<RunListing>,
) -> Vec<ListingRow> {
    let start = state
        .listings
        .refreshing
        .lock()
        .map(|mut set| set.insert(target.id.clone()))
        .unwrap_or(false);
    if start {
        let (state, target, origin, listings) = (
            state.clone(),
            target.clone(),
            origin.to_owned(),
            listings.clone(),
        );
        tokio::spawn(async move {
            refresh_remote_rows(&state, &target, &origin, &listings).await;
            if let Ok(mut set) = state.listings.refreshing.lock() {
                set.remove(&target.id);
            }
        });
    }
    let cache = state.listings.rows.lock().await;
    listings
        .into_iter()
        .map(|l| {
            // Keep the cached summary, but show the target's current listing fields.
            let Some(cached) = cache.get(&(target.id.clone(), l.run_id.clone())) else {
                return ListingRow::new(l, None, None);
            };
            let mut summary = cached.row.summary.clone();
            if let Some(summary) = summary
                .as_mut()
                .filter(|_| cached.stamp != ListingStamp::remote(&l))
            {
                summary.mark_stale();
            }
            ListingRow {
                listing: l,
                summary,
                context: None,
            }
        })
        .collect()
}

async fn refresh_remote_rows(
    state: &Arc<AppState>,
    target: &Target,
    origin: &str,
    listings: &[RunListing],
) {
    let stale: Vec<(RunListing, Option<RunData>)> = {
        let cache = state.listings.rows.lock().await;
        listings
            .iter()
            .filter_map(
                |l| match cache.get(&(target.id.clone(), l.run_id.clone())) {
                    Some(c) if c.stamp == ListingStamp::remote(l) => None,
                    other => Some((l.clone(), other.map(|c| c.data.clone()))),
                },
            )
            .collect()
    };
    let refreshed: Vec<(String, Option<CachedListing>)> =
        futures::stream::iter(stale.into_iter().map(|(listing, data)| {
            let state = state.clone();
            let origin = origin.to_owned();
            let target_id = target.id.clone();
            async move {
                let mut data = data.unwrap_or_else(|| RunData {
                    run_id: listing.run_id.clone(),
                    ..Default::default()
                });
                let after = data.events.last().map(|e| e.seq).unwrap_or(0);
                let stamp = ListingStamp::remote(&listing);
                let run_id = listing.run_id.clone();
                let entry = match state.remote.read(&origin, &run_id, after).await {
                    Ok((stored, events)) => {
                        merge_events(&mut data, stored, events, after);
                        let view = projection::project(&target_id, &data, now_ms());
                        let row = ListingRow::new(listing, Some(&view), None);
                        Some(CachedListing { stamp, data, row })
                    }
                    Err(_) => None,
                };
                (run_id, entry)
            }
        }))
        .buffer_unordered(REMOTE_READS_AT_ONCE)
        .collect()
        .await;
    let mut cache = state.listings.rows.lock().await;
    for (id, entry) in refreshed {
        if let Some(entry) = entry {
            cache.insert((target.id.clone(), id), entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::RunSummary;
    use crate::projection;
    use crate::source::RunData;
    use crate::test_events::{completed, event, failed, log, started, terminal};

    /// The worker fails at 6 000 and the graph ends; a postmortem keeps Zeroshot closing the run
    /// until 85 000 (the late-failure run of the teardown: "1m 25s" in the list).
    fn late_failure(closed: bool) -> RunData {
        let mut events = vec![
            log(1, None, 1_000, "system", "Preparing execution environment"),
            started(2, 1, "worker", 1),
            log(
                3,
                Some(1),
                6_000,
                "error",
                "Node worker failed: crash after 4.0s: boom",
            ),
            completed(4, 1, "worker", failed("crash")),
            event(
                5,
                json!({"kind": "primary_graph_completed", "result": {"status": "failed", "reason": "worker_failed"}}),
            ),
            log(6, None, 7_000, "system", "Active Memory postmortem started"),
            log(
                7,
                None,
                84_000,
                "system",
                "Active Memory postmortem completed",
            ),
        ];
        if closed {
            events.push(log(8, None, 85_000, "error", "Run failed: worker_failed"));
            events.push(terminal(9, "failed", "worker_failed"));
        }
        RunData {
            run_id: "01a1a5e8-c91b-7073-b3e9-fdbd462600af".into(),
            admitted: json!({"graph": {"root": {"kind": "step", "name": "worker", "worker": "builtin.agent.worker@1"}}, "runtime": {"harness": "claude", "provider": "anthropic"}}),
            snapshot: json!({"phase": if closed { "finished" } else { "running" }, "terminal": Value::Null}),
            events,
            ..Default::default()
        }
    }

    #[test]
    fn the_list_counts_to_the_outcome_like_the_header() {
        for closed in [false, true] {
            let view = projection::project("local", &late_failure(closed), 100_000);
            let summary = RunSummary::of(&view);
            // The header: `endedAt - startedAt` (run-model.ts `runElapsed`).
            let header = view.ended_at.zip(view.started_at).map(|(e, s)| e - s);
            assert_eq!(summary.duration_ms, header, "closed: {closed}");
            assert_eq!(
                summary.duration_ms,
                Some(5_000),
                "the postmortem is not the run's duration (closed: {closed})"
            );
        }
    }
}
