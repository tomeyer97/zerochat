//! `zsf init`: a new feature directory, its feature branch and (optionally) its plan.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::model::{EventKind, FeatureConfig, Plan, SLICE_TRAILER, SliceStatus};
use crate::store::Feature;
use crate::time::now_ms;

pub struct InitOptions {
    pub name: String,
    pub repo: PathBuf,
    pub branch: String,
    pub base: String,
    /// An existing plan.toml (spec paths relative to it).
    pub plan: Option<PathBuf>,
    /// A refined spec directory to import.
    pub refined: Option<PathBuf>,
    pub checkpoints: Vec<String>,
    /// Applied to the generated `feature.toml` (`FeatureConfig::new` plus defaults) before it is
    /// written: `zsf init` sets the launch graph, runtime, program and `max_parallel` here.
    pub configure: Box<dyn FnOnce(&mut FeatureConfig)>,
}

pub struct Initialized {
    pub feature: Feature,
    pub messages: Vec<String>,
}

pub fn init(root: &Path, options: InitOptions) -> Result<Initialized> {
    if !crate::model::valid_id(&options.name) {
        bail!("feature names are letters, digits, '.', '_' or '-'");
    }
    let dir = root.join(&options.name);
    if dir.join("feature.toml").exists() {
        bail!("{} already exists", dir.display());
    }
    let repo = options
        .repo
        .canonicalize()
        .with_context(|| format!("no repository at {}", options.repo.display()))?;
    crate::git::git(&repo, &["rev-parse", "--git-dir"]).context("not a git repository")?;
    let mut messages = Vec::new();
    if crate::git::branch_exists(&repo, &options.branch) {
        messages.push(format!("using the existing branch {}", options.branch));
    } else {
        crate::git::rev_parse(&repo, &options.base)
            .with_context(|| format!("base {} is not a commit", options.base))?;
        crate::git::git(&repo, &["branch", &options.branch, &options.base])?;
        messages.push(format!("created {} at {}", options.branch, options.base));
    }
    let mut config = FeatureConfig::new(
        options.name.clone(),
        repo,
        options.branch.clone(),
        options.base.clone(),
    );
    (options.configure)(&mut config);
    std::fs::create_dir_all(&dir)?;
    let feature = Feature {
        dir: dir.clone(),
        config,
    };
    feature.save_config()?;
    if let Some(refined) = &options.refined {
        let imported = crate::import::import_into(&feature, refined, &options.checkpoints)?;
        messages.extend(imported.warnings);
    } else if let Some(plan_path) = &options.plan {
        let plan = read_plan(plan_path, &options.checkpoints)?;
        feature.save_plan(&plan)?;
    } else {
        feature.save_plan(&Plan::default())?;
        messages.push(
            "the plan is empty: add slices with `zsf import <refined dir> --feature NAME`"
                .to_owned(),
        );
    }
    let merged = mark_merged_from_branch(&feature)?;
    if !merged.is_empty() {
        messages.push(format!(
            "already on {}: {}",
            feature.config.branch,
            merged.join(", ")
        ));
    }
    feature.append_event(&crate::model::Event {
        ts: now_ms(),
        slice: None,
        attempt: None,
        kind: EventKind::Initialized,
        run_id: None,
        detail: String::new(),
    })?;
    let plan = feature.load_plan()?;
    let checkpoints: Vec<&str> = plan
        .slices
        .iter()
        .filter(|s| s.checkpoint)
        .map(|s| s.id.as_str())
        .collect();
    let checkpoints = if checkpoints.is_empty() {
        "none".to_owned()
    } else {
        checkpoints.join(", ")
    };
    messages.push(format!(
        "initialized {}: {} slices, checkpoints {checkpoints}",
        dir.display(),
        plan.slices.len(),
    ));
    Ok(Initialized { feature, messages })
}

/// A plan.toml given to `init`: spec paths made absolute (they are relative to the file) and the
/// `--checkpoints` marked. `Feature::save_plan` validates it.
fn read_plan(path: &Path, checkpoints: &[String]) -> Result<Plan> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut plan: Plan =
        toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))?;
    let base = path
        .canonicalize()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    plan.rebase_specs(&base);
    plan.mark_checkpoints(checkpoints);
    Ok(plan)
}

/// Slices whose `Slice: <id>` commit is already on the branch (a re-init) count as merged.
pub fn mark_merged_from_branch(feature: &Feature) -> Result<Vec<String>> {
    let plan = feature.load_plan()?;
    let repo = feature.resolve(&feature.config.repo);
    let range = format!(
        "{}..refs/heads/{}",
        feature.config.base, feature.config.branch
    );
    // A base the repository does not know yet (not fetched) means nothing is merged.
    let commits = crate::git::commits(&repo, &[&range]).unwrap_or_default();
    let mut state = feature.load_state()?;
    let mut merged = Vec::new();
    for commit in &commits {
        for slice in &plan.slices {
            if !crate::jobs::has_trailer(&commit.message, SLICE_TRAILER, &slice.id) {
                continue;
            }
            let s = state.slices.entry(slice.id.clone()).or_default();
            if s.status == SliceStatus::Pending && s.attempts.is_empty() {
                s.status = SliceStatus::Merged;
                s.commit = Some(commit.sha.clone());
                s.approved = true;
                merged.push(slice.id.clone());
            }
        }
    }
    if !merged.is_empty() {
        feature.save_state(&mut state)?;
    }
    Ok(merged)
}
