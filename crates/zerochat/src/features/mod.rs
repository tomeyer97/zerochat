//! Features: work run by zsf (Zeroshot features, the `zsf` crate in this repository), the
//! slice/feature scheduler. A feature is a spec cut into slices with dependencies between them;
//! zsf runs one Zeroshot run per slice attempt and merges each finished slice onto the feature
//! branch. zerochat reads zsf's state through the zsf library and changes it only through
//! `zsf::daemon::submit` (queued to the feature's daemon, the one writer). Everything here is
//! synchronous; the server calls it on blocking threads.
//!
//! Every slice and feature carries the list of actions zsf would accept in its current state
//! (`actions`, from `zsf::actions`: zerochat does not restate zsf's rules); the UI offers exactly
//! that list, and `act` refuses anything outside it before asking zsf.
//!
//! - [`view`]: what the API answers (the serialised views)
//! - [`project`]: zsf's plan, state and events projected into those views
//! - [`runs`]: the Zeroshot runs behind slice attempts (status, cost)
//! - [`files`]: tasks, artifacts, events and the git reads of one feature or slice
//! - [`actions`]: what a person does to a feature or a slice
//! - [`run_context`]: a run's slice and attempt, for the run page and the sidebar
//! - [`git`], [`line_diff`]: the git plumbing and the task diff underneath

mod actions;
mod files;
mod git;
mod line_diff;
mod project;
mod run_context;
mod runs;
mod view;

#[cfg(test)]
pub mod fixture;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use anyhow::Result;
use zsf::FeatureFacts;
use zsf::store::Feature;

// The module's API, by the paths the server, the context providers, the type generator
// (`api_types.rs`) and the goldens use. Some are only used by tests and the type generator.
#[allow(unused_imports)]
pub use actions::{ActError, ActOutcome, ActionBody, act};
#[allow(unused_imports)]
pub use files::{artifact, changes, commit, events, safe_relative, task, worktree_diff};
#[allow(unused_imports)]
pub use line_diff::line_diff;
#[cfg(test)]
pub use project::feature_actions;
#[allow(unused_imports)]
pub use project::{slice_actions, waiting_for_you};
pub use run_context::{SliceIndex, context_label, context_line};
pub use runs::{Roots, RunFactsCache};
#[allow(unused_imports)]
pub use view::{
    AttemptView, AutoNoteView, ChangeStat, CommitView, FeatureChanges, FeatureCommit,
    FeatureDetail, FeatureRow, FileStat, RunRef, SliceView, TaskView, WorktreeDiff,
};

use project::{Loaded, feature_needs_person, offered_feature_actions, row, slice_views};

/// zsf's state root: `zsf_state_dir` from zerochat's config, else zsf's own resolution.
pub fn zsf_root(config: &crate::config::Config) -> PathBuf {
    config
        .zsf_state_dir
        .clone()
        .unwrap_or_else(zsf::store::state_root)
}

/// The `zsf` program for starting a daemon: `zsf_bin`, else beside this executable, else on PATH.
pub fn zsf_bin(config: &crate::config::Config) -> Option<PathBuf> {
    if let Some(bin) = &config.zsf_bin {
        return bin.is_file().then(|| bin.clone());
    }
    beside_current_exe("zsf").or_else(|| on_path("zsf"))
}

/// `program` in the directory of the running executable, if it is there.
fn beside_current_exe(program: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(program);
    path.is_file().then_some(path)
}

/// `program` in the first directory of `PATH` that has it.
fn on_path(program: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
}

/// Features under `root` (directories with a `feature.toml`).
pub fn names(root: &Path) -> Vec<String> {
    zsf::store::list_features(root)
}

/// There is a feature `name` under `root`.
fn has_feature(root: &Path, name: &str) -> bool {
    names(root).iter().any(|n| n == name)
}

/// The feature `name`, `None` when there is no such feature (an error when it is there but
/// cannot be opened).
fn open_feature(root: &Path, name: &str) -> Result<Option<Feature>> {
    if !has_feature(root, name) {
        return Ok(None);
    }
    Ok(Some(Feature::open(root, name)?))
}

/// [`open_feature`], `None` too when `slice` is not a valid slice id.
fn open_slice(root: &Path, name: &str, slice: &str) -> Result<Option<Feature>> {
    if !zsf::model::valid_id(slice) {
        return Ok(None);
    }
    open_feature(root, name)
}

/// The Zeroshot state directories the features' runs live in.
pub fn zeroshot_roots(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for name in names(root) {
        if let Ok(feature) = Feature::open(root, &name) {
            let dir = zsf::store::zeroshot_state_root(&feature.config.env);
            if !out.contains(&dir) {
                out.push(dir);
            }
        }
    }
    out
}

/// What the readers need besides the feature itself.
pub struct Ctx<'a> {
    pub root: &'a Path,
    pub roots: &'a Roots,
    pub runs: &'a RunFactsCache,
    pub zsf_bin: Option<&'a Path>,
    pub now: u64,
}

/// Every feature that can be opened, read (features that cannot are left out).
fn load_all(root: &Path) -> impl Iterator<Item = Loaded> {
    names(root)
        .into_iter()
        .filter_map(|name| Feature::open(root, &name).ok())
        .map(Loaded::read)
}

/// Every feature, by name.
pub fn list(ctx: &Ctx) -> Vec<FeatureRow> {
    load_all(ctx.root)
        .map(|loaded| {
            let slices = slice_views(ctx, &loaded, false);
            row(&loaded, &slices)
        })
        .collect()
}

/// One feature with its slices, or `None` when there is no such feature.
pub fn detail(ctx: &Ctx, name: &str) -> Result<Option<FeatureDetail>> {
    let Some(feature) = open_feature(ctx.root, name)? else {
        return Ok(None);
    };
    let loaded = Loaded::read(feature);
    let slices = slice_views(ctx, &loaded, true);
    let row = row(&loaded, &slices);
    let config = &loaded.feature.config;
    // Unlike the row's, this counts a checkpoint waiting for approval as not finished yet.
    let all_finished = loaded.plan.as_ref().is_some_and(|plan| {
        let finished = |id: &str, checkpoint: bool| {
            loaded
                .state
                .slices
                .get(id)
                .is_some_and(|s| s.display(checkpoint).is_finished())
        };
        !plan.slices.is_empty() && plan.slices.iter().all(|p| finished(&p.id, p.checkpoint))
    });
    let repo = loaded.feature.resolve(&config.repo);
    let branch_commit = git::rev_parse(&repo, &format!("refs/heads/{}", config.branch));
    let facts = FeatureFacts {
        all_finished,
        daemon_alive: row.daemon_pid.is_some(),
        can_spawn: ctx.zsf_bin.is_some(),
    };
    let actions = offered_feature_actions(&loaded.state, facts, loaded.issue.is_none())
        .into_iter()
        .map(zsf::FeatureAction::as_str)
        .collect();
    Ok(Some(FeatureDetail {
        dir: loaded.feature.dir.display().to_string(),
        max_parallel: config.limits.max_parallel,
        poll_secs: config.limits.poll_secs,
        branch_commit,
        actions,
        slices,
        row,
    }))
}

/// How many features need a person now (for the nav badge), read without the runs.
pub fn needs_you_count(root: &Path) -> usize {
    load_all(root).filter(feature_needs_person).count()
}

/// The types of this module the web reads (`web/src/generated/features.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, features: FeatureRow, FeatureDetail, SliceView, AutoNoteView, AttemptView, RunRef, ChangeStat, FileStat, TaskView,
        CommitView, WorktreeDiff, FeatureCommit, FeatureChanges)
}

/// This module's value sets in `web/src/generated/shapes.ts` (api_types.rs `shape_fragments`):
/// slice states here, the actions in `TS_ACTION_SHAPES` (written after benchmarks' fragment).
#[cfg(test)]
pub(crate) const TS_SHAPES: &str = r#"/** zsf's `SliceStatus` (lowercase): `waiting_for_review` is a checkpoint whose run is done, not merged; `waiting_for_approval` a checkpoint an older zsf merged before its review. */
export type SliceStatus =
  | 'pending'
  | 'launching'
  | 'running'
  | 'integrating'
  | 'waiting_for_review'
  | 'merged'
  | 'waiting_for_approval'
  | 'failed'
  | 'interrupted'
  | 'skipped';

/** zsf's `AttemptResult` (snake_case): how a slice attempt ended. */
export type AttemptResult =
  | 'merged'
  | 'gate_failed'
  | 'empty'
  | 'conflict'
  | 'quality_failed'
  | 'stopped'
  | 'infra_failed'
  | 'launch_failed'
  | 'integration_failed'
  | 'abandoned'
  | 'skipped'
  | 'changes_requested';
"#;

/// The feature and slice actions in `web/src/generated/shapes.ts` (see `TS_SHAPES`).
#[cfg(test)]
pub(crate) const TS_ACTION_SHAPES: &str = r#"/** What zsf accepts on a feature now (`FeatureDetail::actions`). */
export type FeatureAction = 'start' | 'stop' | 'unpause' | 'max_parallel';
/** What zsf accepts on a slice now (`SliceView::actions`). */
export type SliceAction = 'approve' | 'retry' | 'accept' | 'skip' | 'edit';
"#;
