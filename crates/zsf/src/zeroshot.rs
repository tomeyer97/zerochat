//! The Zeroshot boundary: start a run, read its status, resume or force-stop it. [`ZeroshotCli`]
//! is the real one (the `zeroshot` CLI plus a read-only look at the run's ledger); tests use
//! their own implementation of [`Zeroshot`].

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension};
use serde_json::Value;

use crate::hooks::{HookEnv, tail};

/// Everything needed to start one slice attempt.
#[derive(Clone, Debug)]
pub struct LaunchRequest {
    pub title: String,
    pub worktree: PathBuf,
    pub task_file: PathBuf,
    /// `{"task": "<task file text>"}` for `--input`.
    pub input_file: PathBuf,
    pub graph: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
    pub args: Vec<String>,
    /// The launch hook, replacing the default `zeroshot run`.
    pub hook: Option<String>,
    pub env: HookEnv,
    /// Receives the launch command's output.
    pub log_file: PathBuf,
    pub timeout: Duration,
    /// Idempotency key (`--submission-key`): launching twice with it yields one run.
    pub submission_key: Option<String>,
}

/// What a run's ledger and controller say.
#[derive(Clone, Debug, PartialEq)]
pub enum RunStatus {
    Running,
    /// The run ended; `status` is Zeroshot's terminal status (`succeeded`, `failed`, …, or
    /// [`UNKNOWN_STATUS`] when the ledger has none) and `reason` its terminal reason
    /// (`change_attempts_exhausted`, `force_stopped`, …), which the engine interprets.
    Finished {
        status: String,
        reason: Option<String>,
    },
    /// The run is not finished but its controller is gone (crash, reboot).
    Gone {
        why: String,
    },
    /// No ledger or no controller yet: just launched, or never started.
    NotStarted,
    /// The ledger could not be read this time.
    Unreadable {
        why: String,
    },
}

pub trait Zeroshot: Send + Sync {
    /// Starts the run detached from zsf and returns its run id.
    fn launch(&self, request: &LaunchRequest) -> Result<String>;
    fn status(&self, run_id: &str) -> RunStatus;
    /// `zeroshot resume <run>`; returns the successor's run id.
    fn resume(&self, run_id: &str, cwd: &Path, env: &HookEnv, log_file: &Path) -> Result<String>;
    fn force_stop(&self, run_id: &str) -> Result<()>;
    /// The admitted run launched with this submission key, if any. Crash recovery depends on it:
    /// a writer that died between the launch and saving the run id finds the run by its key.
    fn find_run(&self, submission_key: &str) -> Option<String>;
    /// The admitted successor `zeroshot resume` made of this run, if any. Crash recovery depends
    /// on it: a writer that died before saving the successor's id adopts it instead of resuming
    /// the run a second time.
    fn successor(&self, run_id: &str) -> Option<String>;
}

/// The terminal status reported for a finished run whose ledger does not say how it ended.
pub const UNKNOWN_STATUS: &str = "unknown";
/// How long `zeroshot resume` may take to print the successor's receipt and exit.
const RESUME_TIMEOUT: Duration = Duration::from_secs(180);
/// How often a launch or resume is polled for its run id and exit.
const LAUNCH_POLL: Duration = Duration::from_millis(150);
/// Characters of a launch's or resume's output kept in an error (from the end).
const LOG_TAIL: usize = 400;
/// Characters of `zeroshot force-stop`'s error output kept in an error (from the end).
const FORCE_STOP_TAIL: usize = 300;
/// How long `zeroshot status` may take to record a dead controller's result.
const OBSERVE_TIMEOUT: Duration = Duration::from_secs(30);
const OBSERVE_POLL: Duration = Duration::from_millis(50);

/// The `zeroshot` CLI and the local state directory it writes.
pub struct ZeroshotCli {
    /// The `zeroshot` program: stock Zeroshot (16.1 or later, npm
    /// `@the-open-engine-company/zeroshot`); zsf uses only its documented commands.
    pub program: String,
    pub state_root: PathBuf,
    /// The feature's `env` (e.g. `ZEROSHOT_STATE_DIR`), set on every `zeroshot` command.
    pub env: Vec<(String, String)>,
    caps: OnceLock<Caps>,
}

/// Optional `zeroshot run` flags, detected once from `zeroshot run --help` (see `caps`): zsf
/// passes a flag only when this Zeroshot lists it.
#[derive(Clone, Copy, Debug, Default)]
struct Caps {
    detach: bool,
    /// `--label k=v`, a label stored on the run so it can be found again. Not in upstream
    /// Zeroshot (16.1 has no such flag); zsf tags its runs with it when a Zeroshot offers it.
    label: bool,
    /// `--submission-key`: an idempotent launch.
    submission_key: bool,
}

impl ZeroshotCli {
    pub fn new(program: impl Into<String>, state_root: PathBuf) -> Self {
        Self {
            program: program.into(),
            state_root,
            env: Vec::new(),
            caps: OnceLock::new(),
        }
    }

    /// Sets the environment of every `zeroshot` command (the feature's `env`).
    pub fn with_env(mut self, env: Vec<(String, String)>) -> Self {
        self.env = env;
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null());
        command
    }

    /// `zeroshot status <run>`, bounded: a dead controller's run stays unfinished in the ledger
    /// until a `zeroshot` command looks at it; that look writes its terminal result (succeeded
    /// when the graph had finished, failed otherwise, which makes it resumable).
    fn observe(&self, run_id: &str) {
        let Ok(mut child) = self
            .command()
            .arg("status")
            .arg(run_id)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let deadline = Instant::now() + OBSERVE_TIMEOUT;
        while Instant::now() < deadline {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(OBSERVE_POLL);
        }
        let _ = child.kill();
        reap(child);
    }

    /// Which optional flags `zeroshot run --help` lists (a plain text search of the help).
    fn caps(&self) -> Caps {
        *self.caps.get_or_init(|| {
            let help = self
                .command()
                .args(["run", "--help"])
                .stderr(Stdio::null())
                .output();
            match help {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    Caps {
                        detach: text.contains("--detach"),
                        label: text.contains("--label"),
                        submission_key: text.contains("--submission-key"),
                    }
                }
                Err(_) => Caps::default(),
            }
        })
    }

    fn run_dir(&self, run_id: &str) -> Option<PathBuf> {
        (!run_id.is_empty() && !run_id.contains(['/', '\\', '.']))
            .then(|| self.state_root.join(ledger::RUNS_DIR).join(run_id))
    }
}

impl Zeroshot for ZeroshotCli {
    fn launch(&self, req: &LaunchRequest) -> Result<String> {
        let mut command = match &req.hook {
            Some(hook) => {
                let mut c = Command::new("bash");
                c.arg("-c").arg(hook);
                c
            }
            None => {
                let mut c = Command::new(&self.program);
                c.arg("run").arg("--title").arg(&req.title);
                if let Some(graph) = &req.graph {
                    c.arg("--graph").arg(graph);
                }
                if let Some(runtime) = &req.runtime {
                    c.arg("--runtime-config").arg(runtime);
                }
                c.arg("--input").arg(&req.input_file);
                let caps = self.caps();
                if caps.detach {
                    c.arg("--detach");
                }
                if caps.submission_key
                    && let Some(key) = &req.submission_key
                {
                    c.arg("--submission-key").arg(key);
                }
                if caps.label {
                    for (k, v) in [
                        ("zsf.feature", req.env.feature.clone()),
                        ("zsf.slice", req.env.slice.clone()),
                        ("zsf.attempt", req.env.attempt.to_string()),
                    ] {
                        c.arg("--label").arg(format!("{k}={v}"));
                    }
                }
                c.args(&req.args);
                c
            }
        };
        let log = File::create(&req.log_file)
            .with_context(|| format!("cannot write {}", req.log_file.display()))?;
        command
            .current_dir(&req.worktree)
            .envs(req.env.vars())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        crate::sys::detach(&mut command);
        let child = command.spawn().map_err(|e| {
            anyhow!(
                "cannot start the launch ({}): {e}",
                if req.hook.is_some() {
                    "bash"
                } else {
                    self.program.as_str()
                }
            )
        })?;
        let read = || std::fs::read_to_string(&req.log_file).unwrap_or_default();
        // A detached launch may keep running after it printed the receipt.
        let status = match wait_child(child, req.timeout, || find_run_id(&read()).is_some())? {
            Waited::Done => return Ok(find_run_id(&read()).unwrap_or_default()),
            Waited::TimedOut => bail!(
                "no run id within {}s: {}",
                req.timeout.as_secs(),
                tail(&read(), LOG_TAIL)
            ),
            Waited::Exited(status) => status,
        };
        let output = read();
        if let Some(id) = find_run_id(&output) {
            return Ok(id);
        }
        // The key was used before (a launch repeated after a crash) for a run whose
        // submission differs a little (e.g. regenerated context): that run is ours.
        if req.submission_key.is_some()
            && let Some(id) = already_admitted(&output)
        {
            return Ok(id);
        }
        bail!(
            "the launch exited ({status}) without printing a run id{}",
            if output.trim().is_empty() {
                " (no output)".to_owned()
            } else {
                format!(": {}", tail(&output, LOG_TAIL))
            }
        );
    }

    fn status(&self, run_id: &str) -> RunStatus {
        let Some(dir) = self.run_dir(run_id) else {
            return RunStatus::Unreadable {
                why: format!("invalid run id {run_id:?}"),
            };
        };
        let status = local_status(&dir, crate::sys::boot_time_secs());
        if !matches!(status, RunStatus::Gone { .. }) {
            return status;
        }
        // The controller may have died after the graph finished (e.g. during the Active Memory
        // postmortem): let Zeroshot record what the run came to, then look again.
        self.observe(run_id);
        match local_status(&dir, crate::sys::boot_time_secs()) {
            finished @ RunStatus::Finished { .. } => finished,
            _ => status,
        }
    }

    fn resume(&self, run_id: &str, cwd: &Path, env: &HookEnv, log_file: &Path) -> Result<String> {
        // The dead controller's agents may still be working in the worktree; the successor must
        // not share it with them.
        if let Some(dir) = self.run_dir(run_id) {
            crate::sys::stop_processes_mentioning(&format!("{}/", dir.display()));
        }
        let log = File::create(log_file)?;
        let mut command = Command::new(&self.program);
        command
            .arg("resume")
            .arg(run_id)
            .current_dir(cwd)
            .envs(env.vars())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        crate::sys::detach(&mut command);
        let child = command
            .spawn()
            .map_err(|e| anyhow!("cannot start {}: {e}", self.program))?;
        // Unlike a launch, the resume is waited for until it exits: its exit status counts.
        let status = match wait_child(child, RESUME_TIMEOUT, || false)? {
            Waited::Exited(status) => status,
            Waited::Done | Waited::TimedOut => bail!(
                "zeroshot resume did not answer within {}s",
                RESUME_TIMEOUT.as_secs()
            ),
        };
        let text = std::fs::read_to_string(log_file).unwrap_or_default();
        match find_run_id(&text) {
            Some(id) if status.success() && id != run_id => Ok(id),
            _ => Err(anyhow!(
                "zeroshot resume failed ({status}): {}",
                tail(&text, LOG_TAIL)
            )),
        }
    }

    fn force_stop(&self, run_id: &str) -> Result<()> {
        let out = self.command().arg("force-stop").arg(run_id).output()?;
        if !out.status.success() {
            bail!(
                "zeroshot force-stop: {}",
                tail(&String::from_utf8_lossy(&out.stderr), FORCE_STOP_TAIL)
            );
        }
        Ok(())
    }

    fn find_run(&self, submission_key: &str) -> Option<String> {
        let runs = std::fs::read_dir(self.state_root.join(ledger::RUNS_DIR)).ok()?;
        let needle = serde_json::to_string(submission_key).ok()?;
        for entry in runs.flatten() {
            let dir = entry.path();
            // The recovery document (written before the controller starts) names the key; the
            // run counts only once its ledger admitted it.
            let Ok(text) = std::fs::read_to_string(dir.join(ledger::RECOVERY_FILE)) else {
                continue;
            };
            if !text.contains(&needle) {
                continue;
            }
            let doc: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            if ledger::submission_key(&doc) != Some(submission_key) {
                continue;
            }
            if let Ok(Some(stored)) = read_stored(&dir.join(ledger::LEDGER_FILE))
                && let Some(id) = ledger::run_id(&stored)
            {
                return Some(id.to_owned());
            }
        }
        None
    }

    fn successor(&self, run_id: &str) -> Option<String> {
        let recovery = self.run_dir(run_id)?.join(ledger::RECOVERY_FILE);
        let text = std::fs::read_to_string(recovery).ok()?;
        let doc: Value = serde_json::from_str(&text).ok()?;
        let successor = ledger::successor_run_id(&doc)?;
        // Zeroshot records the successor before admitting it; only an admitted one counts.
        let successor_ledger = self.run_dir(successor)?.join(ledger::LEDGER_FILE);
        matches!(read_stored(&successor_ledger), Ok(Some(_))).then(|| successor.to_owned())
    }
}

/// Zeroshot's private on-disk layout under its state directory, as far as zsf reads it: every
/// file name and JSON path in one place, for when Zeroshot changes them. Read-only.
/// Public for the integration tests, which write ledgers with the same names.
pub mod ledger {
    use serde_json::Value;

    /// `<state dir>/runs/<run id>/`: one directory per run.
    pub const RUNS_DIR: &str = "runs";
    /// The run's ledger (SQLite); its one `v2_runs` row holds the stored run as JSON.
    pub const LEDGER_FILE: &str = "runs.sqlite3";
    pub const STORED_RUN_QUERY: &str = "SELECT stored_json FROM v2_runs LIMIT 1";
    /// Per-run recovery document: the submission (with its key), the workspace, and resume
    /// lineage (`resumedFrom`, `successorRunId`).
    pub const RECOVERY_FILE: &str = "workspace-recovery.json";
    /// Written by a live controller: `{"pid": …, "socket": "…"}`.
    pub const CONTROLLER_READY_FILE: &str = "controller.ready.json";

    // The recovery document.

    pub fn submission_key(recovery: &Value) -> Option<&str> {
        recovery["submission"]["submissionKey"].as_str()
    }

    pub fn successor_run_id(recovery: &Value) -> Option<&str> {
        recovery["successorRunId"].as_str()
    }

    // The stored run (the ledger row).

    pub fn run_id(stored: &Value) -> Option<&str> {
        stored["snapshot"]["runId"].as_str()
    }

    /// `snapshot.terminal`: set once the run ended (`{"status": …, "reason": …}`).
    pub fn terminal(stored: &Value) -> Option<&Value> {
        Some(&stored["snapshot"]["terminal"]).filter(|t| t.is_object())
    }

    pub fn terminal_status(terminal: &Value) -> Option<&str> {
        terminal["status"].as_str()
    }

    pub fn terminal_reason(terminal: &Value) -> Option<&str> {
        terminal["reason"].as_str()
    }

    /// `snapshot.phase` is `finished` (an ended run, sometimes before `terminal` is written).
    pub fn phase_finished(stored: &Value) -> bool {
        stored["snapshot"]["phase"].as_str() == Some("finished")
    }

    // The controller's ready file.

    pub fn controller_pid(ready: &Value) -> u32 {
        ready["pid"].as_u64().unwrap_or(0) as u32
    }

    pub fn controller_socket(ready: &Value) -> Option<&str> {
        ready["socket"].as_str()
    }
}

/// The run a repeated launch's submission key already belongs to: Zeroshot (16.1) rejects the
/// launch with `submission key already identifies a different admitted run (<id>)` in its error
/// text and has no machine-readable form of it, so this matches the message; if Zeroshot rewords
/// it, this finds nothing and the launch fails as an infrastructure failure (retried).
pub fn already_admitted(text: &str) -> Option<String> {
    const MARKER: &str = "identifies a different admitted run (";
    let start = text.find(MARKER)? + MARKER.len();
    let id = &text[start..start + text[start..].find(')')?];
    (!id.is_empty() && !id.contains(char::is_whitespace)).then(|| id.to_owned())
}

/// How [`wait_child`] ended.
enum Waited {
    /// The child exited.
    Exited(std::process::ExitStatus),
    /// `done` said so while the child still ran (it is left to finish in the background).
    Done,
    /// Neither within the timeout (the child is left to finish in the background).
    TimedOut,
}

/// Polls `child` until `done()` says the wait is over, the child exits, or `timeout` passes.
fn wait_child(
    mut child: std::process::Child,
    timeout: Duration,
    done: impl Fn() -> bool,
) -> Result<Waited> {
    let deadline = Instant::now() + timeout;
    loop {
        if done() {
            reap(child);
            return Ok(Waited::Done);
        }
        if let Some(status) = child.try_wait()? {
            return Ok(Waited::Exited(status));
        }
        if Instant::now() >= deadline {
            reap(child);
            return Ok(Waited::TimedOut);
        }
        std::thread::sleep(LAUNCH_POLL);
    }
}

/// Waits for a detached child in the background so it never lingers as a zombie.
fn reap(mut child: std::process::Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// The run id in the first line that carries one: `{"runId":"…"}` (the JSON receipt `zeroshot
/// run` and `zeroshot resume` print first, in 16.1). A line that is not JSON as a whole (a
/// launch hook may print the receipt inside other text) is searched for `"runId":"…"`.
pub fn find_run_id(text: &str) -> Option<String> {
    const RUN_ID_KEY: &str = "\"runId\":\"";
    for line in text.lines() {
        let line = line.trim();
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(line) {
            if let Some(id) = map
                .get("runId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                return Some(id.to_owned());
            }
            continue;
        }
        if let Some(start) = line.find(RUN_ID_KEY) {
            let rest = &line[start + RUN_ID_KEY.len()..];
            if let Some(end) = rest.find('"')
                && end > 0
            {
                return Some(rest[..end].to_owned());
            }
        }
    }
    None
}

/// Reads the run's ledger (read-only) and the controller's ready file.
pub fn local_status(run_dir: &Path, boot_time_secs: Option<u64>) -> RunStatus {
    let ledger_file = run_dir.join(ledger::LEDGER_FILE);
    if !ledger_file.is_file() {
        return RunStatus::NotStarted;
    }
    let stored = match read_stored(&ledger_file) {
        Ok(Some(v)) => v,
        Ok(None) => return RunStatus::NotStarted,
        Err(e) => {
            return RunStatus::Unreadable {
                why: format!("{e:#}"),
            };
        }
    };
    if let Some(terminal) = ledger::terminal(&stored) {
        let status = ledger::terminal_status(terminal).unwrap_or(UNKNOWN_STATUS);
        return RunStatus::Finished {
            status: status.to_owned(),
            reason: ledger::terminal_reason(terminal).map(str::to_owned),
        };
    }
    if ledger::phase_finished(&stored) {
        return RunStatus::Finished {
            status: UNKNOWN_STATUS.to_owned(),
            reason: None,
        };
    }
    let ready_path = run_dir.join(ledger::CONTROLLER_READY_FILE);
    let Ok(text) = std::fs::read_to_string(&ready_path) else {
        return RunStatus::NotStarted;
    };
    let ready: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let pid = ledger::controller_pid(&ready);
    if let (Some(boot), Ok(meta)) = (boot_time_secs, std::fs::metadata(&ready_path)) {
        let written = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        if written.is_some_and(|w| w < boot) {
            return RunStatus::Gone {
                why: format!("its controller (pid {pid}) ran before the last reboot"),
            };
        }
    }
    if !crate::sys::pid_alive(pid) {
        return RunStatus::Gone {
            why: format!("its controller (pid {pid}) is not running"),
        };
    }
    if let Some(socket) = ledger::controller_socket(&ready)
        && !Path::new(socket).exists()
    {
        return RunStatus::Gone {
            why: format!("its controller socket {socket} is gone"),
        };
    }
    RunStatus::Running
}

/// Tries to read a ledger this many times while Zeroshot holds it locked.
const READ_ATTEMPTS: u32 = 4;
/// The wait before the first retry; each further wait is three times longer.
const RETRY_DELAY: Duration = Duration::from_millis(40);

/// The stored run in a ledger (`None` when it has no run yet), retried while SQLite says the
/// database is busy or locked (Zeroshot writing it right then).
fn read_stored(path: &Path) -> Result<Option<Value>> {
    let mut delay = RETRY_DELAY;
    let mut attempt = 1;
    loop {
        match read_stored_once(path) {
            Err(e) if attempt < READ_ATTEMPTS && is_locked(&e) => {
                std::thread::sleep(delay);
                delay *= 3;
                attempt += 1;
            }
            result => return result,
        }
    }
}

fn read_stored_once(path: &Path) -> Result<Option<Value>> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)?;
    connection.busy_timeout(Duration::from_secs(2))?;
    let text: Option<String> = connection
        .query_row(ledger::STORED_RUN_QUERY, [], |row| row.get(0))
        .optional()?;
    Ok(match text {
        Some(text) => Some(serde_json::from_str(&text)?),
        None => None,
    })
}

/// SQLite's busy or locked error: another connection is writing.
fn is_locked(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<rusqlite::Error>()
        .and_then(rusqlite::Error::sqlite_error_code)
        .is_some_and(|code| matches!(code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_run_ids() {
        assert_eq!(
            find_run_id("starting\n{\"runId\":\"abc\",\"x\":1}\n{\"runId\":\"zzz\"}"),
            Some("abc".into())
        );
        assert_eq!(
            find_run_id("{\"subscriptionId\":\"s\"}\nprefix {\"runId\":\"r1\"} suffix"),
            Some("r1".into())
        );
        assert_eq!(find_run_id("no id\n{\"runId\":\"\"}"), None);
    }

    fn ledger(dir: &Path, stored: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let c = Connection::open(dir.join("runs.sqlite3")).unwrap();
        c.execute_batch("CREATE TABLE v2_runs (stored_json TEXT)")
            .unwrap();
        c.execute("INSERT INTO v2_runs VALUES (?1)", [stored])
            .unwrap();
    }

    #[test]
    fn reads_terminal_results_and_dead_controllers() {
        let tmp = tempfile::tempdir().unwrap();
        let done = tmp.path().join("done");
        ledger(
            &done,
            r#"{"snapshot":{"phase":"finished","terminal":{"status":"failed","reason":"change_attempts_exhausted"}}}"#,
        );
        assert_eq!(
            local_status(&done, None),
            RunStatus::Finished {
                status: "failed".into(),
                reason: Some("change_attempts_exhausted".into())
            }
        );

        let live = tmp.path().join("live");
        ledger(&live, r#"{"snapshot":{"phase":"running","terminal":null}}"#);
        assert_eq!(local_status(&live, None), RunStatus::NotStarted);
        let me = std::process::id();
        std::fs::write(
            live.join("controller.ready.json"),
            format!(r#"{{"pid":{me}}}"#),
        )
        .unwrap();
        assert_eq!(local_status(&live, None), RunStatus::Running);
        // A ready file older than the boot: the controller died with the reboot.
        assert!(matches!(
            local_status(&live, Some(u64::MAX / 2)),
            RunStatus::Gone { .. }
        ));
        std::fs::write(live.join("controller.ready.json"), r#"{"pid":999999999}"#).unwrap();
        assert!(matches!(local_status(&live, None), RunStatus::Gone { .. }));
        assert_eq!(
            local_status(&tmp.path().join("missing"), None),
            RunStatus::NotStarted
        );
    }
}
