//! Reading a run: its definition, its ledger events and what this machine can see of it. Two
//! sources:
//!
//! - the local Zeroshot state directory: the run's SQLite ledger and the files next to it
//!   ([`RunDir`]), read here;
//! - a direct target: the target's own HTTP API, read by [`Remote`] (`remote.rs`).
//!
//! Whether a run's controller and agents are still running is read from the operating system
//! (`host.rs`). Some runs belong to pb (ProgramBench runner), the owner's tool that runs
//! Zeroshot graphs as benchmarks: pb runs Zeroshot inside a Docker sandbox, so those runs'
//! processes live in another pid namespace that this machine cannot inspect.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

mod host;
mod remote;
mod run_dir;

#[cfg(test)]
mod remote_replay_tests;

pub use host::agent_processes;
// The goldens name it by this path.
#[cfg(test)]
pub use host::is_controller_of;
pub(crate) use host::{hostname, process_alive};
pub use remote::Remote;
pub use run_dir::RunDir;

/// One ledger event with its sequence number (a direct target's history cursor is
/// `v2:<seq>`).
#[derive(Clone, Debug)]
pub struct Event {
    pub seq: u64,
    pub body: Value,
}

/// Everything zerochat knows about a run, normalised across sources.
#[derive(Clone, Debug, Default)]
pub struct RunData {
    pub run_id: String,
    pub title: String,
    /// `admitted`: `{title, graph, initialInput, runtime, source}`.
    pub admitted: Value,
    pub snapshot: Value,
    pub events: Vec<Event>,
    pub local: Option<LocalFacts>,
    /// Usage read from a provider's own session log for executions the ledger has no usage
    /// for yet (Zeroshot records usage only when a turn ends).
    pub live_usage: std::collections::HashMap<u64, crate::pricing::Usage>,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalFacts {
    #[serde(skip)]
    pub run_dir: PathBuf,
    pub workspace: Option<String>,
    pub controller_pid: Option<u32>,
    /// Always `controller_liveness.is_alive()`, for the API's `controllerAlive`; read
    /// `controller_liveness` instead, and change both with `set_liveness`.
    pub controller_alive: bool,
    pub controller_liveness: Liveness,
    pub has_checkpoints: bool,
    /// Lineage from Zeroshot's `workspace-recovery.json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resumed_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resumed_as: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<_>", optional)]
    pub replays: Vec<String>,
    /// Provider processes (Claude, Codex, Copilot) still working in the run's workspace, found
    /// only when the controller is gone (orphans keep changing the workspace).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<_>", optional)]
    pub agent_processes: Vec<AgentProcess>,
}

/// Whether a run's controller process is still there.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    Alive,
    #[default]
    Dead,
    /// Stopped (SIGSTOP): the run cannot progress until it is continued.
    Suspended,
    /// zerochat cannot check (no /proc on this OS, or the controller runs where this machine
    /// cannot see it): the run is shown as Zeroshot last recorded it.
    Unknown,
}

impl Liveness {
    /// False only when zerochat knows the controller is gone or stopped.
    pub fn is_alive(self) -> bool {
        matches!(self, Liveness::Alive | Liveness::Unknown)
    }
}

impl LocalFacts {
    pub fn set_liveness(&mut self, liveness: Liveness) {
        self.controller_liveness = liveness;
        self.controller_alive = liveness.is_alive();
    }

    /// Liveness `dead` → `unknown` when the controller runs in another pid namespace (a pb
    /// run, see the module doc): not finding its pid proves nothing. A controller seen alive or
    /// suspended stays as it is.
    pub fn controller_elsewhere(&mut self) {
        if self.controller_liveness == Liveness::Dead {
            self.set_liveness(Liveness::Unknown);
            self.agent_processes.clear();
        }
    }
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentProcess {
    pub pid: u32,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunListing {
    pub run_id: String,
    pub title: String,
    pub phase: String,
    #[ts(type = "RunTerminal | null")]
    pub terminal: Value,
    pub created_at: Option<u64>,
    #[ts(type = "JsonObject | null")]
    pub source: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub issue: Option<String>,
    /// The target's history cursor (direct targets): unchanged means the run is unchanged.
    #[serde(skip)]
    pub cursor: Option<String>,
}

impl RunListing {
    /// A local run whose ledger was read: its title, the snapshot's phase and terminal, and the
    /// admitted run's source.
    fn of_ledger(run_id: &str, title: String, admitted: &Value, snapshot: &Value) -> Self {
        RunListing {
            run_id: run_id.to_owned(),
            title,
            phase: str_at(snapshot, "/phase"),
            terminal: snapshot.get("terminal").cloned().unwrap_or(Value::Null),
            created_at: uuid_v7_millis(run_id),
            source: admitted.get("source").cloned().unwrap_or(Value::Null),
            issue: None,
            cursor: None,
        }
    }

    /// A local run whose ledger could not be read: phase `unavailable`, and why.
    fn unavailable(run_id: &str, issue: String) -> Self {
        RunListing {
            run_id: run_id.to_owned(),
            title: String::new(),
            phase: "unavailable".to_owned(),
            terminal: Value::Null,
            created_at: uuid_v7_millis(run_id),
            source: Value::Null,
            issue: Some(issue),
            cursor: None,
        }
    }
}

// ---------- local ----------

pub fn local_run_ids(state_root: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(state_root.join("runs"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| RunDir::at(entry.path()).ledger().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids
}

/// The run does not exist on its target (never did, or was deleted).
#[derive(Debug)]
pub struct RunNotFound;

impl std::fmt::Display for RunNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "run not found")
    }
}

impl std::error::Error for RunNotFound {}

pub const NEVER_STARTED: &str =
    "Zeroshot never started this run (its submission was refused); nothing ran.";

/// How long a run directory may have a ledger without a run before the submission counts as
/// refused.
const NEVER_STARTED_AFTER_MS: u64 = 60_000;

/// The error [`stored`] gives while the ledger has no run row yet.
const NO_RUN_YET: &str = "no run yet";

/// Read attempts that fail only because Zeroshot is writing are retried this many times, after
/// a pause that starts at `FIRST_RETRY_DELAY_MS` and triples each time (40, 120, 360 ms).
const READ_RETRIES: usize = 3;
const FIRST_RETRY_DELAY_MS: u64 = 40;

/// A run directory whose ledger still has no run a minute after it was created: Zeroshot refused
/// the submission (busy workspace, oversized input) and left the directory behind.
pub fn never_started(error: &anyhow::Error, run_id: &str) -> bool {
    let now = crate::clock::wall_ms();
    has_no_run_yet(error)
        && uuid_v7_millis(run_id).is_some_and(|created| now > created + NEVER_STARTED_AFTER_MS)
}

/// A read that failed only because Zeroshot was writing at that moment.
pub fn is_transient(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}").to_ascii_lowercase();
    text.contains("locked") || text.contains("busy")
}

/// A read that found the ledger without its run row (Zeroshot has not written it yet).
fn has_no_run_yet(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains(NO_RUN_YET)
}

fn open(path: &Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .context("cannot open the run's ledger")?;
    connection.busy_timeout(Duration::from_secs(2))?;
    Ok(connection)
}

fn stored(connection: &Connection) -> Result<Value> {
    let text: Option<String> = connection
        .query_row("SELECT stored_json FROM v2_runs LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    let text = text.ok_or_else(|| anyhow!("the ledger has {NO_RUN_YET}"))?;
    Ok(serde_json::from_str(&text)?)
}

/// Retries reads that fail while Zeroshot is writing (`database is locked`, a run row that is not
/// there yet): a busy run must not look unreadable.
fn with_retry<T>(mut read: impl FnMut() -> Result<T>) -> Result<T> {
    let mut delay = FIRST_RETRY_DELAY_MS;
    for _ in 0..READ_RETRIES {
        match read() {
            Ok(v) => return Ok(v),
            Err(error) if is_transient(&error) || has_no_run_yet(&error) => {
                std::thread::sleep(Duration::from_millis(delay));
                delay *= 3;
            }
            Err(error) => return Err(error),
        }
    }
    read()
}

pub fn local_listing(state_root: &Path, run_id: &str) -> RunListing {
    let ledger = RunDir::new(state_root, run_id).ledger();
    match with_retry(|| open(&ledger).and_then(|c| stored(&c))) {
        Ok(stored) => RunListing::of_ledger(
            run_id,
            str_at(&stored, "/admitted/title"),
            &stored["admitted"],
            &stored["snapshot"],
        ),
        Err(error) => RunListing::unavailable(run_id, unreadable_issue(&error, run_id)),
    }
}

/// Why a run's listing is `unavailable`, in the words the run list shows.
fn unreadable_issue(error: &anyhow::Error, run_id: &str) -> String {
    if never_started(error, run_id) {
        NEVER_STARTED.to_owned()
    } else {
        format!("{error:#}")
    }
}

/// The listing of a run already read into `data`.
pub fn local_listing_from(run_id: &str, data: &RunData) -> RunListing {
    RunListing::of_ledger(run_id, data.title.clone(), &data.admitted, &data.snapshot)
}

/// Reads the definition, the current snapshot and every event after `after`.
pub fn read_local(state_root: &Path, run_id: &str, after: u64) -> Result<(Value, Vec<Event>)> {
    if run_id.is_empty() || run_id.contains(['/', '\\', '.']) {
        bail!("invalid run id");
    }
    let ledger = RunDir::new(state_root, run_id).ledger();
    if !ledger.is_file() {
        return Err(anyhow!(RunNotFound));
    }
    with_retry(|| read_ledger(&ledger, after))
}

fn read_ledger(path: &Path, after: u64) -> Result<(Value, Vec<Event>)> {
    let connection = open(path)?;
    let stored = stored(&connection)?;
    // Zeroshot 16.1 records when each event was written (`recorded_at_ms`); older ledgers lack
    // the column.
    let recorded = connection
        .prepare("SELECT 1 FROM pragma_table_info('v2_run_events') WHERE name = 'recorded_at_ms'")?
        .exists([])?;
    let sql = if recorded {
        "SELECT sequence, event_json, recorded_at_ms FROM v2_run_events WHERE sequence > ?1 ORDER BY sequence"
    } else {
        "SELECT sequence, event_json, NULL FROM v2_run_events WHERE sequence > ?1 ORDER BY sequence"
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([after as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    })?;
    let mut events = Vec::new();
    for row in rows {
        let (seq, text, recorded_at) = row?;
        let mut body = serde_json::from_str(&text).unwrap_or(Value::Null);
        with_recorded_time(&mut body, recorded_at);
        events.push(Event {
            seq: seq as u64,
            body,
        });
    }
    Ok((stored, events))
}

/// An event without a time of its own (`at`, or a log line's `timestamp`) gets the time
/// Zeroshot recorded it at as `at`: node starts and ends then have exact times.
pub fn with_recorded_time(body: &mut Value, recorded_at: Option<i64>) {
    let Some(at) = recorded_at.and_then(|t| u64::try_from(t).ok()) else {
        return;
    };
    if let Some(map) = body.as_object_mut()
        && !map.contains_key("at")
        && !map.contains_key("timestamp")
    {
        map.insert("at".to_owned(), Value::from(at));
    }
}

/// What the files next to a run's ledger say: its workspace and lineage
/// (`workspace-recovery.json`), its controller and whether it still runs
/// (`controller.ready.json`), and whether it has checkpoints.
pub fn local_facts(state_root: &Path, run_id: &str) -> LocalFacts {
    let dir = RunDir::new(state_root, run_id);
    let recovery = read_json(&dir.recovery()).unwrap_or(Value::Null);
    let text = |key: &str| recovery.get(key).and_then(Value::as_str).map(str::to_owned);
    // A successor that was claimed but never admitted leaves no run directory; ignore it.
    let resumed_as =
        text("successorRunId").filter(|id| RunDir::new(state_root, id).ledger().is_file());
    let replays = recovery
        .get("replays")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let controller_pid = read_json(&dir.controller_ready())
        .and_then(|ready| ready.get("pid").and_then(Value::as_u64))
        .map(|pid| pid as u32);
    let controller_liveness = match controller_pid {
        Some(pid) => host::controller_liveness(pid, run_id),
        None => Liveness::Dead,
    };
    LocalFacts {
        workspace: text("workspace"),
        controller_pid,
        controller_alive: controller_liveness.is_alive(),
        controller_liveness,
        has_checkpoints: dir.checkpoint_index().is_file(),
        resumed_from: text("resumedFrom"),
        resumed_as,
        replays,
        agent_processes: Vec::new(),
        run_dir: dir.into_path(),
    }
}

/// A JSON file's content; `None` when it is missing or not JSON.
fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Size and modification time of a run's ledger files: unchanged means the run is unchanged.
pub fn ledger_stamp(state_root: &Path, run_id: &str) -> Vec<(u64, u128)> {
    let dir = RunDir::new(state_root, run_id);
    [dir.ledger(), dir.ledger_wal()]
        .iter()
        .map(|path| file_stamp(path))
        .collect()
}

/// `(size, modified in ns since the epoch)`; zeros for what cannot be read.
fn file_stamp(path: &Path) -> (u64, u128) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (0, 0);
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    (metadata.len(), modified.map(|d| d.as_nanos()).unwrap_or(0))
}

// ---------- helpers ----------

/// Unix ms encoded in a UUIDv7.
pub fn uuid_v7_millis(id: &str) -> Option<u64> {
    let hex: String = id.chars().filter(|c| *c != '-').take(12).collect();
    (hex.len() == 12)
        .then(|| u64::from_str_radix(&hex, 16).ok())
        .flatten()
}

/// The string at a JSON pointer, or `""`.
pub fn str_at(value: &Value, pointer: &str) -> String {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The types of this module the web reads (`web/src/generated/source.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, source: Liveness, AgentProcess, LocalFacts, RunListing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v7_time() {
        assert_eq!(
            uuid_v7_millis("01a1a5e8-c91b-7073-b3e9-fdbd462600af"),
            Some(0x01a1a5e8c91b)
        );
    }

    #[test]
    fn recorded_time_fills_in_only_a_missing_time() {
        let mut started =
            serde_json::json!({"kind": "node_started", "reference": {"execution": 1}});
        with_recorded_time(&mut started, Some(1795175269036));
        assert_eq!(started["at"], 1795175269036u64);
        let mut fork = serde_json::json!({"kind": "node_started", "at": 5});
        with_recorded_time(&mut fork, Some(9));
        assert_eq!(fork["at"], 5, "an event's own time wins");
        let mut log = serde_json::json!({"kind": "safe_log", "timestamp": 7});
        with_recorded_time(&mut log, Some(9));
        assert!(
            log.get("at").is_none(),
            "a log line keeps its own timestamp"
        );
        let mut old = serde_json::json!({"kind": "node_started"});
        with_recorded_time(&mut old, None);
        assert!(
            old.get("at").is_none(),
            "ledgers without recorded_at_ms stay as they are"
        );
    }

    #[test]
    fn reads_recorded_at_ms_from_a_16_1_ledger() {
        let state = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/recorded/zeroshot16/state");
        let (_, events) = read_local(&state, "01a1f8c8-acbc-7ed0-b0bf-40743f8ad243", 0).unwrap();
        let started = events
            .iter()
            .find(|e| e.body["kind"] == "node_started")
            .unwrap();
        assert_eq!(started.body["at"], 1795175269036u64);
        let fork =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/recorded/fork/state");
        let (_, events) = read_local(&fork, "01a1f8c8-7bb2-7fc3-a632-427dfefb25c8", 0).unwrap();
        assert!(
            events.iter().any(|e| e.body["kind"] == "node_started"),
            "a ledger without the column still reads"
        );
    }
}
