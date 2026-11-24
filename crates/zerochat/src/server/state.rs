//! What every handler shares: where things are, the zeroshot CLI, and the caches of the
//! subsystems (runs, run lists, benchmarks, CLI facts), each owned by the module that uses it.
//!
//! Locks: a cache that async code reads, or that stays locked across an `.await` (the run cache
//! while it reads a run, the run list while it refreshes, the CLI version while it asks), uses
//! `tokio::sync::Mutex`; a cache only touched for a moment from synchronous code uses
//! `std::sync::Mutex`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::actions::Cli;
use crate::config::{self, Config};
use crate::context::{self, RunContext, RunContextProvider};
use crate::discovery::{self, Target, TargetKind};
use crate::features;
use crate::paths;
use crate::source::Remote;

use super::benchmarks_state::BenchmarksState;
use super::boundary::HostAllowlist;
use super::error::ApiError;
use super::listings::Listings;
use super::run_store::RunStore;

/// The target that shows feature slices' runs kept in Zeroshot state directories of their own.
pub(crate) const ZSF_TARGET: &str = "zsf";
/// How long a target's CLI capabilities are reused before asking the CLI again.
const CAPABILITIES_FRESH_FOR: Duration = Duration::from_secs(60);
/// How long the zeroshot version is reused before asking the CLI again.
const VERSION_FRESH_FOR: Duration = Duration::from_secs(300);

pub struct AppState {
    /// Zeroshot's local state directory (the `local` target's runs).
    pub state_root: PathBuf,
    /// Zeroshot's own config directory (`targets.json`).
    pub config_root: PathBuf,
    /// Where zerochat's `config.toml` (run sources, `pb_root`, zsf) lives.
    pub zerochat_config_root: PathBuf,
    pub home: PathBuf,
    pub cli: Cli,
    pub remote: Remote,
    pub allowed_hosts: HostAllowlist,
    /// Runs read so far, and Claude session-log usage (`run_store`).
    pub(crate) runs: RunStore,
    /// Run-list rows (`listings`).
    pub(crate) listings: Listings,
    /// pb query results and the batches this server started (`benchmarks_state`).
    pub(crate) benchmarks: BenchmarksState,
    /// What each target's zeroshot CLI can do, and its version.
    cli_facts: CliFacts,
    /// Status and cost of feature slices' runs, by ledger stamp.
    pub(crate) run_facts: Arc<features::RunFactsCache>,
}

#[derive(Default)]
struct CliFacts {
    capabilities: Mutex<HashMap<String, (Instant, Vec<String>)>>,
    version: Mutex<Option<(Instant, Option<String>)>>,
    /// zsf's `--version`, by the program asked.
    zsf_version: Mutex<Option<(Instant, PathBuf, Option<String>)>>,
}

impl AppState {
    pub fn new(
        state_root: PathBuf,
        config_root: PathBuf,
        zerochat_config_root: PathBuf,
        cli: Cli,
        allowed_hosts: HostAllowlist,
    ) -> Self {
        Self {
            state_root,
            config_root,
            zerochat_config_root,
            home: paths::home(),
            cli,
            remote: Remote::new(),
            allowed_hosts,
            runs: RunStore::default(),
            listings: Listings::default(),
            benchmarks: BenchmarksState::default(),
            cli_facts: CliFacts::default(),
            run_facts: Arc::new(features::RunFactsCache::default()),
        }
    }

    /// zerochat's `config.toml`, read again on every call so an edit takes effect without a
    /// restart. A handler that needs several settings reads it once and passes the snapshot on.
    pub(crate) fn config(&self) -> Config {
        config::load(&self.zerochat_config_root)
    }

    /// Zeroshot's own targets (`local`, `targets.json`, re-read each time) plus the sources
    /// configured in `config`, plus the zsf target when feature slices ran elsewhere.
    pub(crate) fn all_targets(&self, config: &Config) -> (Vec<Target>, Option<String>) {
        let (mut targets, zeroshot_issue) = discovery::targets(&self.config_root);
        targets.extend(config.sources.clone());
        if let Some(zsf) = self.zsf_target(config, &targets) {
            targets.push(zsf);
        }
        let issue = match (zeroshot_issue, config.issue.clone()) {
            (Some(a), Some(b)) => Some(format!("{a}; {b}")),
            (a, b) => a.or(b),
        };
        (targets, issue)
    }

    /// Feature slices may run in a Zeroshot state directory of their own (`[env]` of the
    /// feature). Those not already read as another target become one read-only target, so their
    /// runs open like any other.
    fn zsf_target(&self, config: &Config, targets: &[Target]) -> Option<Target> {
        if targets.iter().any(|t| t.id == ZSF_TARGET) {
            return None;
        }
        let known: Vec<PathBuf> = targets
            .iter()
            .flat_map(|t| t.roots.clone())
            .chain([self.state_root.clone()])
            .collect();
        let roots: Vec<PathBuf> = features::zeroshot_roots(&features::zsf_root(config))
            .into_iter()
            .filter(|r| !known.contains(r) && r.is_dir())
            .collect();
        if roots.is_empty() {
            return None;
        }
        Some(Target {
            id: ZSF_TARGET.to_owned(),
            name: ZSF_TARGET.to_owned(),
            kind: TargetKind::Observe,
            origin: None,
            cli_name: None,
            roots,
            actions_allowed: false,
        })
    }

    pub(crate) fn target(&self, id: &str) -> Result<Target, ApiError> {
        let (targets, _) = self.all_targets(&self.config());
        targets.into_iter().find(|t| t.id == id).ok_or_else(|| {
            ApiError::not_found(
                "target_not_found",
                format!("No target `{id}` is configured."),
            )
        })
    }

    /// Where a run can be read, local first: `(target id, state root)`.
    pub(crate) fn run_roots(&self, config: &Config) -> features::Roots {
        let mut roots = vec![(discovery::LOCAL_TARGET.to_owned(), self.state_root.clone())];
        for target in self.all_targets(config).0 {
            if target.kind == TargetKind::Observe {
                roots.extend(target.roots.iter().map(|r| (target.id.clone(), r.clone())));
            }
        }
        roots
    }

    pub(crate) async fn capabilities(&self, target: &Target) -> Vec<String> {
        // Configured sources (`Observe`) are read-only: zerochat drives no controller there and
        // refuses every action (`require_actions`), so they advertise nothing.
        if target.kind == TargetKind::Observe {
            return Vec::new();
        }
        let key = target.id.clone();
        if let Some((at, features)) = self.cli_facts.capabilities.lock().await.get(&key)
            && at.elapsed() < CAPABILITIES_FRESH_FOR
        {
            return features.clone();
        }
        let features = self.cli.capabilities(target.cli_name.as_deref()).await;
        self.cli_facts
            .capabilities
            .lock()
            .await
            .insert(key, (Instant::now(), features.clone()));
        features
    }

    pub(crate) async fn version(&self) -> Option<String> {
        let mut cached = self.cli_facts.version.lock().await;
        if let Some((at, version)) = cached.as_ref()
            && at.elapsed() < VERSION_FRESH_FOR
        {
            return version.clone();
        }
        let version = self.cli.version().await;
        *cached = Some((Instant::now(), version.clone()));
        version
    }

    /// `zsf --version` of `program` (`x.y.z`), reused like the zeroshot version.
    pub(crate) async fn zsf_version(
        &self,
        program: &std::path::Path,
        timeout: Duration,
    ) -> Option<String> {
        let mut cached = self.cli_facts.zsf_version.lock().await;
        if let Some((at, asked, version)) = cached.as_ref()
            && asked == program
            && at.elapsed() < VERSION_FRESH_FOR
        {
            return version.clone();
        }
        let mut command = tokio::process::Command::new(program);
        command
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(timeout, command.output())
            .await
            .ok()
            .and_then(Result::ok);
        let version = output
            .filter(|o| o.status.success())
            .and_then(|o| discovery::parse_version(&String::from_utf8_lossy(&o.stdout)))
            .map(|(a, b, c)| format!("{a}.{b}.{c}"));
        *cached = Some((Instant::now(), program.to_path_buf(), version.clone()));
        version
    }

    /// Looks up a run's context (which batch/task/seed it is, how it scored) across every
    /// configured provider. `None` for an ordinary Zeroshot run no tool recognises.
    pub(crate) async fn context(&self, run_id: &str) -> Option<RunContext> {
        let providers = context_providers(&self.config());
        let run_id = run_id.to_owned();
        tokio::task::spawn_blocking(move || context::lookup(&providers, &run_id))
            .await
            .ok()
            .flatten()
    }
}

/// Every configured run-context provider: pb's benchmark runs and zsf's feature slices.
pub(crate) fn context_providers(config: &Config) -> Vec<Box<dyn RunContextProvider>> {
    context::providers(config.pb_root.as_deref(), Some(&features::zsf_root(config)))
}
