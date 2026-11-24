//! Batches zerochat started that pb has not recorded yet, and what a running cell's agent is
//! doing and has spent so far (read live, outside the cached queries).

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::batch::BatchStatus;
use super::detail::BatchDetail;
use super::live_spend::live_spend;
use super::pb_pricing::{Rates, pb_pricing};
use crate::validate::is_plain_name;
use crate::{ledger, paths};

/// How long a batch id `pb run --detach` printed may go with no `batches` row and no new line in
/// its log before it reads as failed to start, when its runner's pid is unknown (pb normally
/// writes the row within seconds; `uv run` building an environment the first time takes longer).
pub const START_GRACE: Duration = Duration::from_secs(180);

/// What zerochat itself knows about a batch it started: when, and the detached runner's pid.
#[derive(Clone, Copy, Debug)]
pub struct StartedHere {
    pub at: SystemTime,
    pub pid: Option<u32>,
}

/// The last lines of pb's own log for a batch (`<pb_root>/batches/<id>/pb.log`: the detached
/// runner's stdout and stderr), at most 12 lines / 2 kB.
pub(super) fn start_log_tail(pb_root: &Path, id: &str) -> Option<String> {
    if !is_plain_name(id) {
        return None;
    }
    let bytes = std::fs::read(paths::pb_batch_log(pb_root, id)).ok()?;
    let tail = &bytes[bytes.len().saturating_sub(4096)..];
    let text = String::from_utf8_lossy(tail);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut out = lines[lines.len().saturating_sub(12)..].join("\n");
    if out.len() > 2048 {
        out = crate::text::tail_utf8(&out, 2048).to_owned();
    }
    (!out.is_empty()).then_some(out)
}

/// A batch id with no `batches` row yet: `pb run --detach` prints the id and returns before its
/// runner has written the row (seconds; longer while `uv run` builds a cold environment). Known
/// when zerochat started it (`started`) or when pb's parent already created
/// `<pb_root>/batches/<id>/` (it does, for `pb.log`, before spawning the runner). The batch is
/// - `starting` while the runner may still be coming up;
/// - `failed_to_start` once the runner's pid is gone, or nothing was heard from it for
///   `START_GRACE`, with the tail of `pb.log` saying why;
/// - `None` for an id nobody started (the caller's 404).
pub fn pending_batch(
    pb_root: &Path,
    id: &str,
    started: Option<StartedHere>,
    now: SystemTime,
) -> Option<BatchDetail> {
    if !is_plain_name(id) || id.starts_with("label:") {
        return None;
    }
    let dir = paths::pb_batch_dir(pb_root, id);
    let dir_time = std::fs::metadata(&dir)
        .ok()
        .filter(|m| m.is_dir())
        .and_then(|m| m.modified().ok());
    let since = started.map(|s| s.at).or(dir_time)?;
    let log_time = std::fs::metadata(paths::pb_batch_log(pb_root, id))
        .and_then(|m| m.modified())
        .ok();
    let last_heard = log_time.map_or(since, |t| t.max(since));
    let runner_gone = started
        .and_then(|s| s.pid)
        .is_some_and(|pid| !crate::source::process_alive(pid));
    let quiet_too_long = now.duration_since(last_heard).unwrap_or_default() > START_GRACE;
    let failed = runner_gone || quiet_too_long;
    let started_ms = since
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64);
    Some(BatchDetail {
        id: id.to_owned(),
        status: if failed {
            BatchStatus::FailedToStart
        } else {
            BatchStatus::Starting
        },
        starting: !failed,
        start_log: start_log_tail(pb_root, id),
        started_at: started_ms,
        ..Default::default()
    })
}

/// What a running cell's agent is doing and has spent so far, read live. Applied by the server to
/// the cached `BatchDetail` JSON on every request rather than inside the cached query: none of it
/// touches pb.sqlite, so the cache would not notice it change. For each `running_agent` cell:
/// - `zeroshotRunId`, when pb has not written it yet, from the run's own Zeroshot state directory
///   (`runs/<run_id>/agent/zeroshot-state/runs/<zeroshot run id>/runs.sqlite3`, where pb's runner
///   finds the ledger). An older pb writes `runs.zeroshot_run_id` only once the agent finished,
///   so its running run could not be opened; a newer one writes it within seconds;
/// - `liveNodes`: the nodes Zeroshot's ledger shows started and not yet completed;
/// - `liveCostUsd`: the shim's per-call usage priced with pb.toml's table, as pb's runner prices
///   it to enforce the cost cap (pb records the run's cost only when the agent finishes), and
///   `liveCostUnpriced`, the models pb.toml has no price for (their tokens are left out).
pub fn fill_live_cells(pb_root: &Path, detail: &mut Value) {
    let Some(cells) = detail.get_mut("cells").and_then(Value::as_array_mut) else {
        return;
    };
    let mut pricing: Option<HashMap<String, Rates>> = None;
    for cell in cells {
        let Ok(key) = RunningCellKey::deserialize(&*cell) else {
            continue;
        };
        let (Some("running_agent"), Some(run_id)) = (key.state.as_deref(), key.run_id) else {
            continue;
        };
        if !is_plain_name(&run_id) {
            continue;
        }
        let pricing = pricing.get_or_insert_with(|| pb_pricing(pb_root));
        let live = live_fields(pb_root, &run_id, key.zeroshot_run_id, pricing);
        if let (Some(cell), Ok(Value::Object(fields))) =
            (cell.as_object_mut(), serde_json::to_value(live))
        {
            cell.extend(fields);
        }
    }
}

/// The `Cell` fields `fill_live_cells` reads (names as `Cell` serialises them).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunningCellKey {
    state: Option<String>,
    run_id: Option<String>,
    zeroshot_run_id: Option<String>,
}

/// The fields `fill_live_cells` adds to a running cell; each one only when known.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct LiveFields {
    /// Set only when the cell had none.
    #[serde(skip_serializing_if = "Option::is_none")]
    zeroshot_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_nodes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_cost_unpriced: Option<Vec<String>>,
}

fn live_fields(
    pb_root: &Path,
    run_id: &str,
    recorded_zeroshot_id: Option<String>,
    pricing: &HashMap<String, Rates>,
) -> LiveFields {
    let mut live = LiveFields::default();
    let zeroshot_run_id = match recorded_zeroshot_id {
        Some(id) => Some(id),
        None => {
            live.zeroshot_run_id = live_zeroshot_run_id(pb_root, run_id);
            live.zeroshot_run_id.clone()
        }
    };
    if let Some(zid) = zeroshot_run_id.filter(|z| is_plain_name(z)) {
        let ledger = paths::ledger_path(&paths::pb_run_state_root(pb_root, run_id), &zid);
        live.live_nodes = ledger::live_nodes(&ledger);
    }
    if let Some(spend) = live_spend(pb_root, run_id, pricing) {
        live.live_cost_usd = Some(spend.cost_usd);
        live.live_cost_unpriced = (!spend.unpriced.is_empty()).then_some(spend.unpriced);
    }
    live
}

fn live_zeroshot_run_id(pb_root: &Path, run_id: &str) -> Option<String> {
    if !is_plain_name(run_id) {
        return None;
    }
    let runs = paths::pb_run_state_root(pb_root, run_id).join("runs");
    let mut ids: Vec<String> = std::fs::read_dir(runs)
        .ok()?
        .flatten()
        .filter(|e| e.path().join(paths::LEDGER_FILE).is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    ids.sort();
    ids.pop()
}
