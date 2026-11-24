//! Where a feature lives on disk and how its files are read and written.
//!
//! ```text
//! $ZSF_STATE_DIR | $XDG_STATE_HOME/zsf | ~/.local/state/zsf
//!   <feature>/
//!     feature.toml   plan.toml   state.json   events.jsonl
//!     context/       copied reference documents (import)
//!     tasks/         <id>-a<n>.md (+ .input.json): the exact task of every attempt
//!     runs/          <id>-a<n>.log: launch output
//!     artifacts/     <id>/…: declared worktree files, saved before a worktree goes
//!     requests/      queued CLI mutations; requests/done/ holds the answers
//!     worktrees/     default slice worktrees
//!     writer.lock    held by the one writer (daemon or a CLI command while no daemon runs)
//!     daemon.pid     daemon.log
//! ```

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::model::{Event, FeatureConfig, Plan, STATE_VERSION, State};
use crate::time::now_ms;

mod requests;
pub use requests::STALE_ANSWER_SECS;

/// `$ZSF_STATE_DIR`, else `$XDG_STATE_HOME/zsf`, else `~/.local/state/zsf`.
pub fn state_root() -> PathBuf {
    state_dir(env, "ZSF_STATE_DIR", "zsf")
}

/// Zeroshot's local state directory: `$ZEROSHOT_STATE_DIR`, else `$XDG_STATE_HOME/zeroshot`,
/// else `~/.local/state/zeroshot` (Zeroshot's own rule; zerochat uses this function for it).
/// The feature's `env` takes precedence over the process environment.
pub fn zeroshot_state_root(feature_env: &std::collections::BTreeMap<String, String>) -> PathBuf {
    let get = |name: &str| {
        feature_env
            .get(name)
            .map(|v| expand_home(v))
            .filter(|v| !v.is_empty())
            .or_else(|| env(name))
    };
    state_dir(get, "ZEROSHOT_STATE_DIR", "zeroshot")
}

/// `$<dir_var>`, else `$XDG_STATE_HOME/<name>`, else `~/.local/state/<name>`.
fn state_dir(get: impl Fn(&str) -> Option<String>, dir_var: &str, name: &str) -> PathBuf {
    if let Some(dir) = get(dir_var) {
        return PathBuf::from(dir);
    }
    if let Some(dir) = get("XDG_STATE_HOME") {
        return PathBuf::from(dir).join(name);
    }
    home().join(".local/state").join(name)
}

pub fn home() -> PathBuf {
    env("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn expand_home(value: &str) -> String {
    match value.strip_prefix("~/") {
        Some(rest) => home().join(rest).to_string_lossy().into_owned(),
        None => value.to_owned(),
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Every feature directory under the state root (those with a `feature.toml`).
pub fn list_features(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("feature.toml").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// One feature's directory and configuration.
#[derive(Clone, Debug)]
pub struct Feature {
    pub dir: PathBuf,
    pub config: FeatureConfig,
}

impl Feature {
    pub fn open(root: &Path, name: &str) -> Result<Feature> {
        if !crate::model::valid_id(name) {
            bail!("invalid feature name {name:?}");
        }
        Self::open_dir(&root.join(name))
    }

    pub fn open_dir(dir: &Path) -> Result<Feature> {
        let path = dir.join("feature.toml");
        let text = fs::read_to_string(&path)
            .with_context(|| format!("no feature at {} (zsf init?)", dir.display()))?;
        let config: FeatureConfig =
            toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))?;
        Ok(Feature {
            dir: dir.to_path_buf(),
            config,
        })
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }
    pub fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }
    pub fn tasks_dir(&self) -> PathBuf {
        self.dir.join("tasks")
    }
    pub fn runs_dir(&self) -> PathBuf {
        self.dir.join("runs")
    }
    pub fn artifacts_dir(&self) -> PathBuf {
        self.dir.join("artifacts")
    }
    pub fn pid_path(&self) -> PathBuf {
        self.dir.join("daemon.pid")
    }
    pub fn daemon_log(&self) -> PathBuf {
        self.dir.join("daemon.log")
    }
    pub fn plan_path(&self) -> PathBuf {
        self.resolve(&self.config.plan)
    }
    pub fn worktrees_dir(&self) -> PathBuf {
        match &self.config.worktrees {
            Some(dir) => self.resolve(dir),
            None => self.dir.join("worktrees"),
        }
    }
    /// The context directory, when there is one.
    pub fn context_dir(&self) -> Option<PathBuf> {
        match &self.config.context {
            Some(dir) => Some(self.resolve(dir)),
            None => Some(self.dir.join("context")).filter(|d| d.is_dir()),
        }
    }

    /// `path` relative to the feature directory unless absolute (`~/` expanded).
    pub fn resolve(&self, path: &Path) -> PathBuf {
        let expanded = PathBuf::from(expand_home(&path.to_string_lossy()));
        if expanded.is_absolute() {
            expanded
        } else {
            self.dir.join(expanded)
        }
    }

    pub fn save_config(&self) -> Result<()> {
        write_atomic(
            &self.dir.join("feature.toml"),
            toml::to_string_pretty(&self.config)?.as_bytes(),
        )
    }

    pub fn load_plan(&self) -> Result<Plan> {
        let path = self.plan_path();
        let text = fs::read_to_string(&path)
            .with_context(|| format!("cannot read the plan {}", path.display()))?;
        let plan: Plan =
            toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))?;
        plan.validate(&self.config.leases)
            .with_context(|| format!("{} is not valid", path.display()))?;
        Ok(plan)
    }

    pub fn save_plan(&self, plan: &Plan) -> Result<()> {
        plan.validate(&self.config.leases)?;
        write_atomic(&self.plan_path(), toml::to_string_pretty(plan)?.as_bytes())
    }

    /// The spec file of a slice (plan paths are relative to the plan file).
    pub fn spec_path(&self, spec: &Path) -> PathBuf {
        let expanded = PathBuf::from(expand_home(&spec.to_string_lossy()));
        if expanded.is_absolute() {
            return expanded;
        }
        self.plan_path()
            .parent()
            .map(|p| p.join(&expanded))
            .unwrap_or(expanded)
    }

    /// The saved state, or a fresh one when there is none yet.
    pub fn load_state(&self) -> Result<State> {
        load_state(&self.state_path())
    }

    pub fn save_state(&self, state: &mut State) -> Result<()> {
        state.version = STATE_VERSION;
        state.updated_ms = now_ms();
        write_atomic(
            &self.state_path(),
            serde_json::to_string_pretty(state)?.as_bytes(),
        )
    }

    pub fn append_event(&self, event: &Event) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events_path())?;
        let mut line = serde_json::to_string(event)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    /// Every event, oldest first (unreadable lines are skipped).
    pub fn events(&self) -> Vec<Event> {
        read_events(&self.events_path())
    }

    // ---------- writer lock and daemon pid

    /// Becomes the writer if nobody else is (non-blocking).
    pub fn try_writer_lock(&self) -> Result<Option<WriterLock>> {
        WriterLock::acquire(&self.dir.join("writer.lock"), false)
    }

    /// Waits until this process is the writer.
    pub fn writer_lock(&self) -> Result<WriterLock> {
        WriterLock::acquire(&self.dir.join("writer.lock"), true)?
            .ok_or_else(|| anyhow!("cannot lock"))
    }

    /// The daemon's pid when it is running.
    pub fn daemon_pid(&self) -> Option<u32> {
        let pid: u32 = fs::read_to_string(self.pid_path())
            .ok()?
            .trim()
            .parse()
            .ok()?;
        crate::sys::pid_alive(pid).then_some(pid)
    }
}

pub fn load_state(path: &Path) -> Result<State> {
    match fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).with_context(|| format!("{} is not valid", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State {
            version: STATE_VERSION,
            ..State::default()
        }),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

pub fn read_events(path: &Path) -> Vec<Event> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect()
}

/// Writes through a temporary file and a rename, so readers see the old or the new file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut file =
            File::create(&tmp).with_context(|| format!("cannot write {}", tmp.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

/// An exclusive `flock` on `writer.lock`, released on drop.
#[derive(Debug)]
pub struct WriterLock {
    _file: File,
}

impl WriterLock {
    fn acquire(path: &Path, wait: bool) -> Result<Option<WriterLock>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .with_context(|| format!("cannot open {}", path.display()))?;
        let flags = if wait {
            libc::LOCK_EX
        } else {
            libc::LOCK_EX | libc::LOCK_NB
        };
        // SAFETY: flock on an fd we own.
        let rc = unsafe { libc::flock(file.as_raw_fd(), flags) };
        if rc == 0 {
            return Ok(Some(WriterLock { _file: file }));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Ok(None);
        }
        Err(error).with_context(|| format!("cannot lock {}", path.display()))
    }
}
