//! The on-disk model: `feature.toml` (config), `plan.toml` (slices), `state.json` (progress,
//! written only by the writer) and `events.jsonl` (one line per transition). Every type here is
//! public and serde so other tools (zerochat) read the same files without duplicating the schema.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- feature.toml

/// `feature.toml`: what the feature is and how its slices run.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureConfig {
    pub name: String,
    /// The git repository the feature is built in.
    pub repo: PathBuf,
    /// The feature branch slices are squashed onto (created from `base` at init).
    pub branch: String,
    pub base: String,
    /// The plan, relative to the feature directory unless absolute.
    #[serde(default = "default_plan")]
    pub plan: PathBuf,
    /// Where slice worktrees go; default `<feature dir>/worktrees`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktrees: Option<PathBuf>,
    /// Reference documents for every slice; default `<feature dir>/context` when it exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<PathBuf>,
    /// First word(s) of squash commit titles and run titles; default the feature name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_prefix: Option<String>,
    #[serde(default)]
    pub launch: LaunchConfig,
    #[serde(default)]
    pub hooks: Hooks,
    #[serde(default)]
    pub limits: Limits,
    /// Named counted resources: `e2e-box = 3`. A slice that lists a lease runs only while
    /// fewer than that many running slices hold it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub leases: BTreeMap<String, u32>,
    /// Worktree paths copied to `artifacts/<slice>/` before a worktree is removed or a slice stops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    /// Environment for hooks and runs (e.g. `ZEROSHOT_STATE_DIR`, `PATH`). `~/` is expanded.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

fn default_plan() -> PathBuf {
    PathBuf::from("plan.toml")
}

impl FeatureConfig {
    /// A new feature's configuration: the four required fields, everything else the default.
    pub fn new(name: String, repo: PathBuf, branch: String, base: String) -> FeatureConfig {
        FeatureConfig {
            name,
            repo,
            branch,
            base,
            plan: default_plan(),
            worktrees: None,
            context: None,
            commit_prefix: None,
            launch: LaunchConfig::default(),
            hooks: Hooks::default(),
            limits: Limits::default(),
            leases: BTreeMap::new(),
            artifacts: Vec::new(),
            env: BTreeMap::new(),
        }
    }
}

/// The default launch: `zeroshot run --title … --graph … --runtime-config … --input …`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchConfig {
    /// The zeroshot program; default `zeroshot` on PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zeroshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PathBuf>,
    /// Extra arguments for `zeroshot run` (e.g. `["--profile", "mine"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// The context is copied into the worktree at this path (git-excluded) so sandboxed agents can
    /// read it; default `.zsf-context`, `""` points the task at the feature's context directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_in_worktree: Option<String>,
}

impl LaunchConfig {
    pub fn program(&self) -> &str {
        self.zeroshot.as_deref().unwrap_or("zeroshot")
    }

    pub fn context_in_worktree(&self) -> Option<&str> {
        match self.context_in_worktree.as_deref() {
            None => Some(".zsf-context"),
            Some("") => None,
            Some(path) => Some(path),
        }
    }
}

/// Shell commands run with `bash -c` and the `ZSF_*` environment. Each is optional.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    /// Replaces the default launch; runs in the worktree and must print a `{"runId":"…"}` line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<String>,
    /// Runs in a fresh worktree before the launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepare: Option<String>,
    /// Must pass (exit 0, in the worktree) before a finished slice is merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// Runs in the worktree after rebasing onto a moved branch tip; failure counts as a conflict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_after_rebase: Option<String>,
    /// Runs in the repository after the branch moved; failure is only reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_merge: Option<String>,
}

impl Hooks {
    /// `self` with every unset hook taken from `fallback`.
    pub fn over(&self, fallback: &Hooks) -> Hooks {
        Hooks {
            launch: self.launch.clone().or_else(|| fallback.launch.clone()),
            prepare: self.prepare.clone().or_else(|| fallback.prepare.clone()),
            gate: self.gate.clone().or_else(|| fallback.gate.clone()),
            check_after_rebase: self
                .check_after_rebase
                .clone()
                .or_else(|| fallback.check_after_rebase.clone()),
            after_merge: self
                .after_merge
                .clone()
                .or_else(|| fallback.after_merge.clone()),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Hooks::default()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// Zeroshot runs at once (launching, running or integrating slices: a conflict re-runs at once).
    pub max_parallel: u32,
    /// No new slice starts while the worktree disk has less free space than this.
    pub min_free_gb: f64,
    /// Relaunches of one slice after infrastructure failures before it stops.
    pub infra_retries: u32,
    /// Re-runs of one slice after a rebase conflict (or failed check after rebase).
    pub conflict_reruns: u32,
    /// Re-runs of one slice after the merge gate failed, with the gate's output as notes.
    pub gate_reruns: u32,
    /// Infrastructure failures in a row (any slices) that pause the feature; 0 never pauses.
    pub pause_after_infra: u32,
    /// Seconds to wait for the launch to print its run id.
    pub launch_timeout_secs: u64,
    /// Seconds a launched run may take to show a live controller before it counts as gone.
    pub start_grace_secs: u64,
    /// Seconds between scheduler ticks of the daemon.
    pub poll_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_parallel: 2,
            min_free_gb: 0.0,
            infra_retries: 2,
            conflict_reruns: 1,
            gate_reruns: 1,
            pause_after_infra: 2,
            launch_timeout_secs: 180,
            start_grace_secs: 300,
            poll_secs: 30,
        }
    }
}

// ---------------------------------------------------------------- plan.toml

/// `plan.toml`: the slices in scheduling order (`[[slice]]` tables).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[serde(default, rename = "slice")]
    pub slices: Vec<SlicePlan>,
}

/// One `[[slice]]` of the plan. Only `id`, `title` and `spec` are required; build one in code
/// with `SlicePlan { id, title, spec, ..Default::default() }`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SlicePlan {
    /// Short id, e.g. `01`; also the `Slice:` commit trailer.
    pub id: String,
    pub title: String,
    /// The slice's task text (Markdown), relative to the plan file unless absolute.
    pub spec: PathBuf,
    #[serde(default)]
    pub deps: Vec<String>,
    /// An approval slice: when its run is done it waits, not merged, for a person's review;
    /// `approve` checks and merges it, and only then do its dependents start.
    #[serde(default, skip_serializing_if = "is_false")]
    pub checkpoint: bool,
    /// Leases (declared in `feature.toml`) the slice holds while it runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leases: Vec<String>,
    /// Squash commit title; default `<prefix> <id>: <title>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_title: Option<String>,
    /// Squash commit body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Per-slice overrides of the feature's launch graph, runtime, arguments and hooks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Hooks::is_empty")]
    pub hooks: Hooks,
}

/// The commit trailer naming the slice a squash commit came from: `Slice: <id>`.
pub const SLICE_TRAILER: &str = "Slice";

fn is_false(value: &bool) -> bool {
    !*value
}

impl Plan {
    pub fn get(&self, id: &str) -> Option<&SlicePlan> {
        self.slices.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut SlicePlan> {
        self.slices.iter_mut().find(|s| s.id == id)
    }

    /// Makes the listed slices checkpoints (others keep what they have).
    pub fn mark_checkpoints(&mut self, ids: &[String]) {
        for slice in &mut self.slices {
            if ids.contains(&slice.id) {
                slice.checkpoint = true;
            }
        }
    }

    /// Joins every relative spec path onto `base` (the directory of the plan file it came from).
    pub fn rebase_specs(&mut self, base: &Path) {
        for slice in &mut self.slices {
            if slice.spec.is_relative() {
                slice.spec = base.join(&slice.spec);
            }
        }
    }

    /// Unique safe ids, known deps, declared leases, no cycles.
    pub fn validate(&self, leases: &BTreeMap<String, u32>) -> Result<()> {
        let mut ids = BTreeSet::new();
        for slice in &self.slices {
            if !valid_id(&slice.id) {
                bail!(
                    "slice id {:?} must be letters, digits, '.', '_' or '-'",
                    slice.id
                );
            }
            if !ids.insert(slice.id.as_str()) {
                bail!("slice {} is listed twice", slice.id);
            }
        }
        for slice in &self.slices {
            for dep in &slice.deps {
                if dep == &slice.id {
                    bail!("slice {} depends on itself", slice.id);
                }
                if !ids.contains(dep.as_str()) {
                    bail!(
                        "slice {} depends on {dep}, which is not in the plan",
                        slice.id
                    );
                }
            }
            for lease in &slice.leases {
                if !leases.contains_key(lease) {
                    bail!(
                        "slice {} needs lease {lease:?}, which feature.toml does not declare under [leases]",
                        slice.id
                    );
                }
            }
        }
        // Cycle check: repeatedly remove slices whose deps are all removed.
        let mut done: BTreeSet<&str> = BTreeSet::new();
        loop {
            let before = done.len();
            for slice in &self.slices {
                if !done.contains(slice.id.as_str())
                    && slice.deps.iter().all(|d| done.contains(d.as_str()))
                {
                    done.insert(&slice.id);
                }
            }
            if done.len() == self.slices.len() {
                return Ok(());
            }
            if done.len() == before {
                let stuck: Vec<_> = self
                    .slices
                    .iter()
                    .filter(|s| !done.contains(s.id.as_str()))
                    .map(|s| s.id.as_str())
                    .collect();
                bail!("dependency cycle among slices {}", stuck.join(", "));
            }
        }
    }
}

/// The longest slice or feature id, and the longest [`slug`].
pub const MAX_ID_LEN: usize = 40;

/// A slice or feature id: non-empty, at most [`MAX_ID_LEN`] bytes, letters, digits, `.`, `_` and
/// `-` only, and not starting with `.` or `-` (ids become file and branch names).
pub fn valid_id(id: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || "._-".contains(c);
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && !id.starts_with(['.', '-'])
        && id.chars().all(allowed)
}

/// `my Title: here` → `my-title-here` (at most [`MAX_ID_LEN`] characters).
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    let mut cut: String = out.chars().take(MAX_ID_LEN).collect();
    while cut.ends_with('-') {
        cut.pop();
    }
    cut
}

// ---------------------------------------------------------------- state.json

pub const STATE_VERSION: u32 = 1;

/// `state.json`: progress of every slice. Written atomically, only by the writer (the daemon,
/// or a CLI command while no daemon runs); readers never need a lock.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    /// The daemon schedules slices (set by `start`, cleared by `stop` and when everything merged).
    pub active: bool,
    /// Why the feature is paused (repeated infrastructure failures); `unpause` clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<String>,
    #[serde(default)]
    pub consecutive_infra: u32,
    /// What the feature waits on when nothing can start (`waiting for you: approve 02`,
    /// `disk below 25 GB`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Slice id → progress. Slices of the plan missing here are pending.
    #[serde(default)]
    pub slices: BTreeMap<String, SliceState>,
    /// Unix ms of the last write.
    #[serde(default)]
    pub updated_ms: u64,
}

// The scheduling rules other tools read the state by. The engine schedules by these same
// functions, so a reader's "waits for 03" and the daemon's decision cannot disagree.
impl State {
    /// Whether `dep` lets its dependents start: skipped, or merged (and, for a checkpoint an older
    /// zsf merged before its review, approved). A dep the plan does not know holds nothing up (a
    /// valid plan has none).
    pub fn dep_done(&self, plan: &Plan, dep: &str) -> bool {
        let checkpoint = plan.get(dep).is_some_and(|p| p.checkpoint);
        match self.slices.get(dep) {
            Some(s) => {
                s.status == SliceStatus::Skipped
                    || (s.status == SliceStatus::Merged && (!checkpoint || s.approved))
            }
            None => plan.get(dep).is_none(),
        }
    }

    /// The deps of a pending `slice` that are not done yet (empty once it left pending).
    pub fn waits_for(&self, plan: &Plan, slice: &SlicePlan) -> Vec<String> {
        let status = self
            .slices
            .get(&slice.id)
            .map(|s| s.status)
            .unwrap_or_default();
        if status != SliceStatus::Pending {
            return Vec::new();
        }
        slice
            .deps
            .iter()
            .filter(|d| !self.dep_done(plan, d))
            .cloned()
            .collect()
    }

    /// `id` waits for a person's review (see [`SliceState::awaiting_approval`]).
    pub fn awaiting_approval(&self, plan: &Plan, id: &str) -> bool {
        let checkpoint = plan.get(id).is_some_and(|p| p.checkpoint);
        self.slices
            .get(id)
            .is_some_and(|s| s.awaiting_approval(checkpoint))
    }

    /// Slices holding a slot (and their leases): see [`SliceStatus::is_active`].
    pub fn active_count(&self) -> usize {
        self.slices
            .values()
            .filter(|s| s.status.is_active())
            .count()
    }

    /// Every slice of the plan is merged or skipped (true for an empty plan).
    pub fn all_finished(&self, plan: &Plan) -> bool {
        plan.slices.iter().all(|p| {
            self.slices
                .get(&p.id)
                .is_some_and(|s| s.status.is_finished())
        })
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum SliceStatus {
    #[default]
    Pending,
    /// Worktree, task file and launch in progress.
    Launching,
    /// A Zeroshot run is working on it.
    Running,
    /// Gate, squash, rebase and fast-forward in progress.
    Integrating,
    /// A checkpoint's run is done: not merged, its worktree kept, waiting for a person to
    /// approve (check and merge), request changes (`retry --notes`) or skip it.
    WaitingForReview,
    Merged,
    /// Stopped; needs a person (`retry`, `accept`, `skip`).
    Failed,
    /// Lost to a pause or a crash; relaunched automatically when the feature runs again.
    Interrupted,
    Skipped,
}

impl SliceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SliceStatus::Pending => "pending",
            SliceStatus::Launching => "launching",
            SliceStatus::Running => "running",
            SliceStatus::Integrating => "integrating",
            SliceStatus::WaitingForReview => "waiting_for_review",
            SliceStatus::Merged => "merged",
            SliceStatus::Failed => "failed",
            SliceStatus::Interrupted => "interrupted",
            SliceStatus::Skipped => "skipped",
        }
    }

    /// Launching, running or integrating: the slice occupies one of the `max_parallel` slots
    /// and holds its leases (an integrating slice keeps its slot: a conflict re-runs it at once).
    pub fn is_active(self) -> bool {
        matches!(
            self,
            SliceStatus::Launching | SliceStatus::Running | SliceStatus::Integrating
        )
    }

    /// Merged or skipped: nothing left to do for this slice.
    pub fn is_finished(self) -> bool {
        matches!(self, SliceStatus::Merged | SliceStatus::Skipped)
    }
}

/// The status a person sees: [`SliceStatus`], with a merged checkpoint that is not approved yet
/// (written by an older zsf, which merged checkpoints before their review) shown as
/// `waiting_for_approval`. Serialises as the same snake_case string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayStatus {
    Pending,
    Launching,
    Running,
    Integrating,
    WaitingForReview,
    Merged,
    WaitingForApproval,
    Failed,
    Interrupted,
    Skipped,
}

impl DisplayStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DisplayStatus::WaitingForApproval => "waiting_for_approval",
            DisplayStatus::Pending => "pending",
            DisplayStatus::Launching => "launching",
            DisplayStatus::Running => "running",
            DisplayStatus::Integrating => "integrating",
            DisplayStatus::WaitingForReview => "waiting_for_review",
            DisplayStatus::Merged => "merged",
            DisplayStatus::Failed => "failed",
            DisplayStatus::Interrupted => "interrupted",
            DisplayStatus::Skipped => "skipped",
        }
    }

    /// Merged (and approved, for a checkpoint) or skipped.
    pub fn is_finished(self) -> bool {
        matches!(self, DisplayStatus::Merged | DisplayStatus::Skipped)
    }
}

impl From<SliceStatus> for DisplayStatus {
    fn from(status: SliceStatus) -> Self {
        match status {
            SliceStatus::Pending => DisplayStatus::Pending,
            SliceStatus::Launching => DisplayStatus::Launching,
            SliceStatus::Running => DisplayStatus::Running,
            SliceStatus::Integrating => DisplayStatus::Integrating,
            SliceStatus::WaitingForReview => DisplayStatus::WaitingForReview,
            SliceStatus::Merged => DisplayStatus::Merged,
            SliceStatus::Failed => DisplayStatus::Failed,
            SliceStatus::Interrupted => DisplayStatus::Interrupted,
            SliceStatus::Skipped => DisplayStatus::Skipped,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SliceState {
    pub status: SliceStatus,
    /// A person approved this checkpoint slice: it merges without being asked again (also after
    /// a conflict re-run); a re-run that changes its work (requested changes, a gate re-run)
    /// clears it. (An older zsf set it on a merged checkpoint, releasing its dependents.)
    #[serde(default, skip_serializing_if = "is_false")]
    pub approved: bool,
    /// Merged by `accept` from a worktree someone finished by hand.
    #[serde(default, skip_serializing_if = "is_false")]
    pub accepted: bool,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    /// Developer notes appended to every later task file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Notes zsf added itself (the previous work after a conflict, the gate's output after it
    /// failed), kept apart from a person's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_notes: Vec<AutoNote>,
    /// A retry asked for while nothing could launch (paused, stopped, every slot busy): the slice
    /// is pending again and launches as a manual retry when the scheduler gets to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<QueuedRetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<PathBuf>,
    /// The slice's working branch (`zsf/<feature branch>/<id>-<slug>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The squash commit on the feature branch, once merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Why the slice stopped (failed, interrupted) or was skipped: the text to show for the
    /// slice. It is the feature-level summary and may differ from the last attempt's
    /// [`Attempt::reason`] (e.g. `infrastructure: <why>` here, `<why>` there); cleared when the
    /// slice runs again, while attempts keep theirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub infra_failures: u32,
    #[serde(default)]
    pub conflict_reruns: u32,
    /// Automatic re-runs after a failed merge gate (up to `[limits] gate_reruns`).
    #[serde(default)]
    pub gate_reruns: u32,
    /// Finished work of an attempt that conflicted, re-applied by the next (re-run) attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carry: Option<Carry>,
    /// Work in progress for this slice (redone when the daemon restarts mid-way).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
}

impl SliceState {
    pub fn last_attempt(&self) -> Option<&Attempt> {
        self.attempts.last()
    }

    pub fn last_attempt_mut(&mut self) -> Option<&mut Attempt> {
        self.attempts.last_mut()
    }

    /// Waits for a person's review before its dependents may start: its run is done and it waits
    /// unmerged ([`Self::waiting_for_review`]), or an older zsf merged it before its review
    /// ([`Self::merged_unapproved`]). `checkpoint` comes from its plan entry.
    pub fn awaiting_approval(&self, checkpoint: bool) -> bool {
        self.waiting_for_review() || self.merged_unapproved(checkpoint)
    }

    /// Waits unmerged for review, with no job on it: a person may approve (check and merge),
    /// request changes (`retry`) or skip it.
    pub fn waiting_for_review(&self) -> bool {
        self.is_idle() && self.status == SliceStatus::WaitingForReview
    }

    /// A checkpoint an older zsf merged before its review, not approved yet: `approve` only
    /// releases its dependents.
    pub fn merged_unapproved(&self, checkpoint: bool) -> bool {
        self.status == SliceStatus::Merged && checkpoint && !self.approved
    }

    /// No job (launch, integration, resume) is working on the slice.
    pub fn is_idle(&self) -> bool {
        self.job.is_none()
    }

    /// Failed or interrupted, with no job on it: a person may retry, accept or skip it.
    pub fn is_stopped(&self) -> bool {
        self.is_idle() && matches!(self.status, SliceStatus::Failed | SliceStatus::Interrupted)
    }

    /// Pending and never attempted: its plan entry may still change.
    pub fn is_editable(&self) -> bool {
        self.is_idle() && self.status == SliceStatus::Pending && self.attempts.is_empty()
    }

    /// The slice's worktree, when it still exists on disk.
    pub fn worktree_on_disk(&self) -> Option<&PathBuf> {
        self.worktree.as_ref().filter(|w| w.is_dir())
    }

    /// The status a person should see (see [`DisplayStatus`]).
    pub fn display(&self, checkpoint: bool) -> DisplayStatus {
        if self.merged_unapproved(checkpoint) {
            DisplayStatus::WaitingForApproval
        } else {
            self.status.into()
        }
    }

    /// [`Self::display`] as its string: `waiting_for_approval` for a merged, unapproved checkpoint.
    pub fn display_status(&self, checkpoint: bool) -> &'static str {
        self.display(checkpoint).as_str()
    }
}

/// A note zsf added to every later task of a slice, with why it added it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(from = "AutoNoteOnDisk")]
pub struct AutoNote {
    pub cause: AutoNoteCause,
    pub text: String,
}

impl AutoNote {
    pub fn new(cause: AutoNoteCause, text: impl Into<String>) -> AutoNote {
        AutoNote {
            cause,
            text: text.into(),
        }
    }
}

/// Why zsf added a note: the slice's work conflicted with the branch tip and was carried over,
/// or the merge gate failed and the slice runs again with its output.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoNoteCause {
    Conflict,
    GateFailed,
}

/// An auto note as `state.json` holds it: `{cause, text}`, or a bare string from an older zsf
/// (whose cause its wording tells: gate notes start with [`GATE_NOTE_START`]).
#[derive(Deserialize)]
#[serde(untagged)]
enum AutoNoteOnDisk {
    Note { cause: AutoNoteCause, text: String },
    Text(String),
}

/// How every gate note (`jobs::gate_note`) starts.
pub const GATE_NOTE_START: &str = "The merge gate failed";

impl From<AutoNoteOnDisk> for AutoNote {
    fn from(note: AutoNoteOnDisk) -> AutoNote {
        match note {
            AutoNoteOnDisk::Note { cause, text } => AutoNote { cause, text },
            AutoNoteOnDisk::Text(text) => {
                let cause = if text.starts_with(GATE_NOTE_START) {
                    AutoNoteCause::GateFailed
                } else {
                    AutoNoteCause::Conflict
                };
                AutoNote { cause, text }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct QueuedRetry {
    #[serde(default)]
    pub fresh: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Carry {
    pub commit: String,
    /// The backup branch holding `commit` (`zsf-backup/<branch>/<id>-a<n>`).
    pub backup: String,
}

/// Work the writer started for a slice.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "job", rename_all = "snake_case")]
pub enum Job {
    Launch {
        kind: AttemptKind,
        #[serde(default)]
        fresh: bool,
    },
    Integrate {
        /// `accept`: merge a worktree finished by hand (no gate, no conflict re-run).
        #[serde(default)]
        accept: bool,
    },
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttemptKind {
    First,
    InfraRetry,
    /// After a rebase conflict, from the new branch tip with the previous work re-applied.
    Rerun,
    ManualRetry,
    /// After the merge gate failed, in the same worktree with the gate's output as notes.
    GateRerun,
}

impl AttemptKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptKind::First => "first",
            AttemptKind::InfraRetry => "infra-retry",
            AttemptKind::Rerun => "rerun",
            AttemptKind::ManualRetry => "manual-retry",
            AttemptKind::GateRerun => "gate-rerun",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub n: u32,
    pub kind: AttemptKind,
    /// A manual retry from a new worktree at the branch tip.
    #[serde(default, skip_serializing_if = "is_false")]
    pub fresh: bool,
    /// The current Zeroshot run (after a resume, the successor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Runs this attempt resumed from, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub earlier_runs: Vec<String>,
    /// The exact task this attempt was given (`tasks/<id>-a<n>.md`).
    pub task_file: PathBuf,
    /// Output of the launch command (`runs/<id>-a<n>.log`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_file: Option<PathBuf>,
    pub started_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<u64>,
    /// When the run was last resumed (its successor started).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<AttemptResult>,
    /// Why this attempt ended the way it did (its [`AttemptResult`] in words). Kept for the
    /// attempt's history; what to show for the slice now is [`SliceState::reason`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// A conflict kept this attempt's work on this branch (`zsf-backup/<branch>/<id>-a<n>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// `zeroshot resume` was tried once for a run whose controller was gone.
    #[serde(default, skip_serializing_if = "is_false")]
    pub resume_tried: bool,
    /// The launch's idempotency key (`zeroshot run --submission-key`), saved before the launch:
    /// a writer that died before recording the run id finds the run by it, and launching again
    /// with the same key returns that run instead of starting a second one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptResult {
    /// The run succeeded and the slice merged.
    Merged,
    /// The run succeeded, but the gate hook failed.
    GateFailed,
    /// The run succeeded without changing anything.
    Empty,
    /// The run succeeded, but the work conflicts with the moved branch tip.
    Conflict,
    /// The review loop ran out of rounds (`change_attempts_exhausted`).
    QualityFailed,
    /// Someone force-stopped the run.
    Stopped,
    /// Agent crash, timeout, controller lost, launch error, …
    InfraFailed,
    LaunchFailed,
    /// Merging failed for another reason (see `reason`).
    IntegrationFailed,
    /// Replaced by a retry before it finished.
    Abandoned,
    /// The slice was skipped while this attempt ran (or before its work merged).
    Skipped,
    /// A person reviewed the finished work and asked for changes (`retry` on a slice waiting
    /// for review): the next attempt makes them.
    ChangesRequested,
}

// ---------------------------------------------------------------- events.jsonl

/// One line of `events.jsonl`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// Unix ms.
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    pub kind: EventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// What an [`Event`] records, as its snake_case `kind`. Grouped by who causes it; several
/// slice outcomes share a name with the [`AttemptResult`] they end an attempt with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    // The feature and its daemon.
    /// `zsf init` created the feature.
    Initialized,
    /// A daemon process took the writer lock.
    DaemonStarted,
    /// The daemon process exited (the feature may still be active: see [`EventKind::Stopped`]).
    DaemonStopped,
    /// The feature was paused after repeated infrastructure failures.
    Paused,
    /// Nothing can start; the detail says what the feature waits on (also `State::note`).
    Waiting,
    /// Every slice is merged or skipped; the feature is no longer active.
    Done,

    // A slice's run.
    /// A Zeroshot run was started (or found again) for an attempt.
    Launched,
    /// The launch failed before a run id was known (counted as an infrastructure failure).
    LaunchFailed,
    /// Zeroshot finished the run; the detail is its terminal status and reason.
    RunFinished,
    /// The review loop gave up (`change_attempts_exhausted`): the slice stops.
    QualityFailed,
    /// The run was force-stopped: the slice stops.
    RunStopped,
    /// An infrastructure failure (crash, timeout, lost controller); relaunched up to a limit.
    InfraFailure,
    /// The run's controller is gone; the slice waits for a resume or a relaunch.
    Interrupted,
    /// A gone run was resumed as a successor run.
    Resumed,
    /// `zeroshot resume` failed.
    ResumeFailed,

    // A slice's integration onto the feature branch.
    /// The gate hook failed: the slice runs once more with the gate's output (up to
    /// `gate_reruns`), or stops.
    GateFailed,
    /// The slice was squashed onto the feature branch.
    Merged,
    /// The work conflicts with the moved branch tip (re-run, or stop).
    Conflict,
    /// Merging failed for another reason (no worktree, no changes, git error).
    IntegrationFailed,
    /// A checkpoint's run is done: it waits, not merged, for a person's review.
    CheckpointWaiting,
    /// A hook whose failure is only reported (`after_merge`) failed.
    HookFailed,

    // Requests (CLI or zerochat), applied by the writer.
    /// `start`: the feature schedules slices.
    Started,
    /// `stop`: the feature stops scheduling (running runs continue).
    Stopped,
    /// `unpause`.
    Unpaused,
    /// `approve`: a checkpoint is checked and merged (an older zsf's merged one is released).
    Approved,
    /// `retry` relaunched (or queued) a stopped slice, or one waiting for review (changes
    /// requested).
    Retried,
    /// `accept` merged a worktree a person finished by hand.
    Accepted,
    /// `skip` dropped a slice.
    Skipped,
    /// `edit` changed a slice, or `max-parallel` the feature.
    Edited,
    /// A request could not be applied; the detail says why.
    RequestRejected,

    /// Written by a newer zsf.
    #[serde(other)]
    Other,
}

impl EventKind {
    /// The snake_case name, as in `events.jsonl`.
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Initialized => "initialized",
            EventKind::DaemonStarted => "daemon_started",
            EventKind::DaemonStopped => "daemon_stopped",
            EventKind::Paused => "paused",
            EventKind::Waiting => "waiting",
            EventKind::Done => "done",
            EventKind::Launched => "launched",
            EventKind::LaunchFailed => "launch_failed",
            EventKind::RunFinished => "run_finished",
            EventKind::QualityFailed => "quality_failed",
            EventKind::RunStopped => "run_stopped",
            EventKind::InfraFailure => "infra_failure",
            EventKind::Interrupted => "interrupted",
            EventKind::Resumed => "resumed",
            EventKind::ResumeFailed => "resume_failed",
            EventKind::GateFailed => "gate_failed",
            EventKind::Merged => "merged",
            EventKind::Conflict => "conflict",
            EventKind::IntegrationFailed => "integration_failed",
            EventKind::CheckpointWaiting => "checkpoint_waiting",
            EventKind::HookFailed => "hook_failed",
            EventKind::Started => "started",
            EventKind::Stopped => "stopped",
            EventKind::Unpaused => "unpaused",
            EventKind::Approved => "approved",
            EventKind::Retried => "retried",
            EventKind::Accepted => "accepted",
            EventKind::Skipped => "skipped",
            EventKind::Edited => "edited",
            EventKind::RequestRejected => "request_rejected",
            EventKind::Other => "other",
        }
    }
}

// ---------------------------------------------------------------- requests

/// A mutation asked for by the CLI (or zerochat): `requests/<id>.json`, applied by the writer,
/// answered in `requests/done/<id>.json` as a [`RequestResult`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Request {
    pub id: String,
    pub ts: u64,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Start,
    Stop,
    Approve {
        slice: String,
    },
    Retry {
        slice: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<String>,
        #[serde(default)]
        fresh: bool,
    },
    Accept {
        slice: String,
    },
    Skip {
        slice: String,
    },
    Unpause,
    SetMaxParallel {
        max_parallel: u32,
    },
    Edit {
        slice: String,
        edit: SliceEdit,
    },
}

impl Action {
    /// The slice the action is about, if any.
    pub fn slice(&self) -> Option<&str> {
        match self {
            Action::Approve { slice }
            | Action::Retry { slice, .. }
            | Action::Accept { slice }
            | Action::Skip { slice }
            | Action::Edit { slice, .. } => Some(slice),
            Action::Start | Action::Stop | Action::Unpause | Action::SetMaxParallel { .. } => None,
        }
    }
}

/// Changes to a slice that has not started yet. Unset fields stay as they are.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SliceEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A new spec file (absolute path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deps: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<bool>,
    /// Move the slice right before this slice in the plan (scheduling order).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RequestResult {
    pub id: String,
    pub ok: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(id: &str, deps: &[&str]) -> SlicePlan {
        SlicePlan {
            id: id.into(),
            title: format!("t{id}"),
            spec: PathBuf::from(format!("{id}.md")),
            deps: deps.iter().map(|d| d.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn auto_notes_read_with_their_cause_and_as_an_older_zsf_wrote_them() {
        let read = |json: &str| serde_json::from_str::<Vec<AutoNote>>(json).unwrap();
        let notes = read(
            r#"[{"cause":"gate_failed","text":"g"},"The merge gate failed on attempt 1: x","The previous attempt's work conflicted"]"#,
        );
        assert_eq!(
            notes.iter().map(|n| n.cause).collect::<Vec<_>>(),
            [
                AutoNoteCause::GateFailed,
                AutoNoteCause::GateFailed,
                AutoNoteCause::Conflict
            ]
        );
        let written = serde_json::to_string(&notes[0]).unwrap();
        assert_eq!(written, r#"{"cause":"gate_failed","text":"g"}"#);
        assert_eq!(read(&format!("[{written}]"))[0], notes[0]);
    }

    #[test]
    fn validates_plans() {
        let ok = Plan {
            slices: vec![slice("01", &[]), slice("02", &["01"])],
        };
        ok.validate(&BTreeMap::new()).unwrap();
        let cycle = Plan {
            slices: vec![slice("01", &["02"]), slice("02", &["01"])],
        };
        assert!(
            cycle
                .validate(&BTreeMap::new())
                .unwrap_err()
                .to_string()
                .contains("cycle")
        );
        let unknown = Plan {
            slices: vec![slice("01", &["09"])],
        };
        assert!(unknown.validate(&BTreeMap::new()).is_err());
        let bad = Plan {
            slices: vec![slice("../x", &[])],
        };
        assert!(bad.validate(&BTreeMap::new()).is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Add the `flag` → API!"), "add-the-flag-api");
        assert!(slug(&"word ".repeat(30)).len() <= 40);
    }

    #[test]
    fn plan_round_trips_through_toml() {
        let mut s = slice("01", &[]);
        s.checkpoint = true;
        s.hooks.gate = Some("make test".into());
        let plan = Plan {
            slices: vec![s, slice("02", &["01"])],
        };
        let text = toml::to_string(&plan).unwrap();
        let back: Plan = toml::from_str(&text).unwrap();
        assert_eq!(back.slices, plan.slices);
    }

    #[test]
    fn unknown_event_kinds_load() {
        let e: Event = serde_json::from_str(r#"{"ts":1,"kind":"something_new"}"#).unwrap();
        assert_eq!(e.kind, EventKind::Other);
    }

    #[test]
    fn requests_are_flat_json() {
        let r = Request {
            id: "x".into(),
            ts: 1,
            action: Action::Retry {
                slice: "02".into(),
                notes: Some("n".into()),
                fresh: true,
            },
        };
        let text = serde_json::to_string(&r).unwrap();
        assert!(text.contains(r#""action":"retry""#), "{text}");
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), r);
    }

    /// `as_str` is written by hand next to `#[serde(rename_all)]`: both must give the same name.
    #[test]
    fn as_str_is_the_serde_name() {
        fn serde_name(value: impl Serialize) -> String {
            serde_json::to_value(value)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        }
        use SliceStatus as S;
        for s in [
            S::Pending,
            S::Launching,
            S::Running,
            S::Integrating,
            S::WaitingForReview,
            S::Merged,
            S::Failed,
            S::Interrupted,
            S::Skipped,
        ] {
            assert_eq!(s.as_str(), serde_name(s));
            assert_eq!(
                DisplayStatus::from(s).as_str(),
                serde_name(DisplayStatus::from(s))
            );
        }
        let waiting = DisplayStatus::WaitingForApproval;
        assert_eq!(waiting.as_str(), serde_name(waiting));
        use AttemptKind as A;
        for k in [
            A::First,
            A::InfraRetry,
            A::Rerun,
            A::ManualRetry,
            A::GateRerun,
        ] {
            assert_eq!(k.as_str(), serde_name(k));
        }
        use EventKind as E;
        for k in [
            E::Initialized,
            E::DaemonStarted,
            E::DaemonStopped,
            E::Paused,
            E::Waiting,
            E::Done,
            E::Launched,
            E::LaunchFailed,
            E::RunFinished,
            E::QualityFailed,
            E::RunStopped,
            E::InfraFailure,
            E::Interrupted,
            E::Resumed,
            E::ResumeFailed,
            E::GateFailed,
            E::Merged,
            E::Conflict,
            E::IntegrationFailed,
            E::CheckpointWaiting,
            E::HookFailed,
            E::Started,
            E::Stopped,
            E::Unpaused,
            E::Approved,
            E::Retried,
            E::Accepted,
            E::Skipped,
            E::Edited,
            E::RequestRejected,
            E::Other,
        ] {
            assert_eq!(k.as_str(), serde_name(k));
        }
    }
}
