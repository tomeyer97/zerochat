//! The run store: runs read so far, by target and run id, read again incrementally (only the
//! events after the cached cursor), and projected into the run view.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::benchmarks;
use crate::clock::now_ms;
use crate::discovery::{Target, TargetKind};
use crate::harness_name::Harness;
use crate::paths;
use crate::pricing::Usage;
use crate::projection::{self, RunView};
use crate::source::{self, RunData};
use crate::transcript;

use super::args::valid_run_id;
use super::error::ApiError;
use super::state::{AppState, ZSF_TARGET};

/// A run read less than this long ago is served from the cache without reading it again.
const RUN_FRESH_FOR: Duration = Duration::from_millis(400);
/// More runs than this and the least recently read one is dropped.
const MAX_CACHED_RUNS: usize = 32;
/// More session logs than this and the usage cache starts over.
const MAX_SESSION_USAGES: usize = 256;

#[derive(Default)]
pub(crate) struct RunStore {
    runs: Mutex<HashMap<(String, String), Cached>>,
    /// Session-log usage by file, valid while the file keeps its length.
    session_usage: std::sync::Mutex<HashMap<PathBuf, (u64, Usage)>>,
}

struct Cached {
    data: RunData,
    fetched: Instant,
}

/// Where a run's history is read from.
enum RunSource {
    /// A state directory on this machine.
    Local(PathBuf),
    /// A direct target's history API.
    Remote(String),
}

impl AppState {
    /// Reads a run incrementally: only events after the cached cursor are fetched.
    pub(crate) async fn run(&self, target: &Target, run_id: &str) -> Result<RunData, ApiError> {
        valid_run_id(run_id)?;
        let key = (target.id.clone(), run_id.to_owned());
        let mut cache = self.runs.runs.lock().await;
        if let Some(cached) = cache.get(&key)
            && cached.fetched.elapsed() < RUN_FRESH_FOR
        {
            return Ok(cached.data.clone());
        }
        let after = cache
            .get(&key)
            .and_then(|c| c.data.events.last())
            .map(|e| e.seq)
            .unwrap_or(0);
        let source = self.locate(target, run_id, after)?;
        let read = match &source {
            RunSource::Local(root) => {
                let (root, id) = (root.clone(), run_id.to_owned());
                tokio::task::spawn_blocking(move || source::read_local(&root, &id, after))
                    .await
                    .map_err(|e| ApiError::internal(anyhow!(e)))?
            }
            RunSource::Remote(origin) => self.remote.read(origin, run_id, after).await,
        };
        let (stored, events) = match read {
            Ok(read) => read,
            Err(error) if error.downcast_ref::<source::RunNotFound>().is_some() => {
                cache.remove(&key);
                return Err(ApiError::not_found(
                    "run_not_found",
                    "Run not found: it does not exist on this target, or it was deleted.",
                ));
            }
            // Zeroshot is writing: show the last history read instead of an error.
            Err(error) if source::is_transient(&error) && cache.contains_key(&key) => {
                return Ok(cache[&key].data.clone());
            }
            Err(error) if source::never_started(&error, run_id) => {
                return Err(ApiError::not_found(
                    "run_never_started",
                    source::NEVER_STARTED,
                ));
            }
            Err(error) => return Err(self.history_unavailable(run_id, error)),
        };
        let entry = cache.entry(key).or_insert_with(|| Cached {
            data: RunData {
                run_id: run_id.to_owned(),
                ..Default::default()
            },
            fetched: Instant::now(),
        });
        merge_events(&mut entry.data, stored, events, after);
        if let RunSource::Local(root) = &source {
            let controller_here =
                trust_controller_check(target, self.config().pb_root.as_deref(), root);
            entry.data.local = Some(local_facts_with_agents(
                root,
                run_id,
                &entry.data.snapshot,
                controller_here,
            ));
        }
        entry.fetched = Instant::now();
        let data = entry.data.clone();
        if cache.len() > MAX_CACHED_RUNS {
            let oldest = cache
                .iter()
                .min_by_key(|(_, c)| c.fetched)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                cache.remove(&oldest);
            }
        }
        Ok(data)
    }

    /// Where `target` keeps `run_id`'s history. A run already read (`after > 0`) is read again
    /// from where it was, even while its ledger is briefly missing.
    fn locate(&self, target: &Target, run_id: &str, after: u64) -> Result<RunSource, ApiError> {
        let not_found = || ApiError::not_found("run_not_found", "Run not found.");
        match target.kind {
            TargetKind::Local => {
                if !paths::ledger_path(&self.state_root, run_id).is_file() && after == 0 {
                    return Err(not_found());
                }
                Ok(RunSource::Local(self.state_root.clone()))
            }
            TargetKind::Observe => {
                let found = target
                    .roots
                    .iter()
                    .find(|r| paths::ledger_path(r, run_id).is_file())
                    .cloned();
                found
                    .or_else(|| (after > 0).then(|| target.roots.first().cloned()).flatten())
                    .map(RunSource::Local)
                    .ok_or_else(not_found)
            }
            TargetKind::Direct => Ok(RunSource::Remote(target.origin.clone().unwrap_or_default())),
            TargetKind::Hosted => Err(ApiError::bad_request(
                "unsupported_target",
                "zerochat cannot read hosted (Platform) targets yet. Use `zeroshot status --target NAME` or the Platform website.",
            )),
        }
    }

    /// A history read that failed for another reason. The full error (with server paths) goes
    /// to the server log; the page gets it with the ledger and state directory paths named.
    fn history_unavailable(&self, run_id: &str, error: anyhow::Error) -> ApiError {
        tracing::warn!(run = run_id, "cannot read run history: {error:#}");
        let ledger = paths::ledger_path(&self.state_root, run_id)
            .display()
            .to_string();
        let message = format!("{:#}", error.root_cause())
            .replace(&ledger, "the ledger")
            .replace(
                &self.state_root.display().to_string(),
                "the state directory",
            );
        ApiError::unavailable(
            "history_unavailable",
            format!("Cannot read this run's history right now: {message}"),
        )
    }

    /// Projects a run; executions the ledger has no usage for yet get their usage from the
    /// provider's own session log when it is on this machine (local Claude runs).
    pub(crate) fn project(&self, target: &Target, mut data: RunData) -> RunView {
        let view = projection::project(&target.id, &data, now_ms());
        if target.kind != TargetKind::Local {
            return view;
        }
        let Some(workspace) = data.local.as_ref().and_then(|l| l.workspace.clone()) else {
            return view;
        };
        let live = self.live_usage(&view, &workspace);
        if live.is_empty() {
            return view;
        }
        data.live_usage = live;
        projection::project(&target.id, &data, now_ms())
    }

    /// Usage from Claude's session logs for the executions the ledger has none for yet.
    fn live_usage(&self, view: &RunView, workspace: &str) -> HashMap<u64, Usage> {
        let mut live = HashMap::new();
        let without_ledger_usage = view.executions.iter().filter(|e| {
            e.usage_source != projection::UsageSource::Ledger
                || (e.usage.total() == 0 && e.state == projection::ExecState::Interrupted)
        });
        for e in without_ledger_usage {
            if Harness::of(e.harness.as_deref()) != Some(Harness::Claude) {
                continue;
            }
            let end = if e.state == projection::ExecState::Running {
                Some(now_ms())
            } else {
                e.ended_at
            };
            let instructions = view
                .graph_nodes
                .iter()
                .find(|n| n.name == e.node)
                .and_then(|n| n.instructions.as_deref());
            let Some(path) = transcript::find_claude_session_of(
                &self.home,
                workspace,
                e.session_id.as_deref(),
                e.started_at,
                end,
                instructions,
            ) else {
                continue;
            };
            if let Some(usage) = self.session_usage(&path) {
                live.insert(e.id, usage);
            }
        }
        live
    }

    fn session_usage(&self, path: &Path) -> Option<Usage> {
        let len = std::fs::metadata(path).ok()?.len();
        if let Some((cached_len, usage)) = self.runs.session_usage.lock().ok()?.get(path)
            && *cached_len == len
        {
            return Some(*usage);
        }
        let usage = transcript::usage_from_claude_session(path).ok()?;
        let mut cache = self.runs.session_usage.lock().ok()?;
        if cache.len() > MAX_SESSION_USAGES {
            cache.clear();
        }
        cache.insert(path.to_owned(), (len, usage));
        Some(usage)
    }
}

/// Applies a history read (the stored run plus the events after `after`) to `data`. Events
/// starting at or before the cursor mean the history was rewritten: they replace the old ones.
pub(crate) fn merge_events(
    data: &mut RunData,
    stored: Value,
    events: Vec<source::Event>,
    after: u64,
) {
    if events.first().is_some_and(|e| e.seq <= after) {
        data.events.clear();
    }
    data.events.extend(events);
    data.title = source::str_at(&stored, "/admitted/title");
    data.admitted = stored["admitted"].clone();
    data.snapshot = stored["snapshot"].clone();
}

/// Whether a target's run controllers live in this machine's process table, so a missing
/// controller pid means the controller died. Not for configured sources (`TargetKind::Observe`):
/// their runs may be controlled from a sandbox or another machine (pb runs Zeroshot in Docker),
/// so a controller zerochat cannot see is not a dead one. The zsf target is the exception: zsf
/// starts its runs on this machine.
pub(crate) fn controller_on_this_host(target: &Target) -> bool {
    target.kind != TargetKind::Observe || target.id == ZSF_TARGET
}

/// Whether "controller not found in /proc" may be read as "controller gone" for runs in
/// `state_root`: always where the controller runs here (`controller_on_this_host`), and for a pb
/// run that pb itself has recorded as over (interrupted, crashed, its batch's runner dead) —
/// such a ledger can be left saying "running" forever, and it must not read as running.
pub(crate) fn trust_controller_check(
    target: &Target,
    pb_root: Option<&Path>,
    state_root: &Path,
) -> bool {
    if controller_on_this_host(target) {
        return true;
    }
    pb_root
        .and_then(|pb| {
            benchmarks::pb_run_of_state_root(pb, state_root)
                .and_then(|run| benchmarks::pb_run_ended(pb, &run))
        })
        .unwrap_or(false)
}

/// Local facts, plus the provider processes still running in the workspace when the run claims
/// to be running but its controller is gone. `controller_here = false`: see
/// `controller_on_this_host` — liveness is `unknown`, never `dead`.
fn local_facts_with_agents(
    state_root: &Path,
    run_id: &str,
    snapshot: &Value,
    controller_here: bool,
) -> source::LocalFacts {
    let mut facts = source::local_facts(state_root, run_id);
    if !controller_here {
        facts.controller_elsewhere();
    }
    if !facts.controller_liveness.is_alive()
        && snapshot["phase"] == "running"
        && let Some(workspace) = &facts.workspace
    {
        facts.agent_processes = source::agent_processes(workspace);
    }
    facts
}
