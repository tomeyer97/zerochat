//! What the features API answers: the serialised views of features, slices and their attempts,
//! and of the git reads behind them. `web/src/generated/features.ts` is generated from these
//! (`make types`). Times are milliseconds since the epoch, as zsf records them.

use serde::Serialize;
use ts_rs::TS;
use zsf::model::{AttemptKind, AttemptResult, AutoNoteCause, Event};

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeatureRow {
    pub name: String,
    pub repo: String,
    pub branch: String,
    pub base: String,
    pub merged: usize,
    pub skipped: usize,
    pub total: usize,
    /// Slices launching, running or integrating.
    pub running: Vec<String>,
    /// Merged checkpoints waiting for a person's approval.
    pub approve: Vec<String>,
    /// Stopped slices that need a person (retry, accept or skip).
    pub failed: Vec<String>,
    pub paused: Option<String>,
    /// When the feature paused (the last `paused` event).
    pub paused_at: Option<u64>,
    /// zsf's note when something holds slices back (`disk below 25 GB …`, `04 waits for lease …`);
    /// zsf's "waiting for you" and "nothing can start" notes are left out (`approve`/`failed`
    /// already say it).
    pub note: Option<String>,
    pub active: bool,
    pub daemon_pid: Option<u32>,
    /// Ever started (a `started` event or an attempt).
    pub started: bool,
    /// Something only a person can unblock: paused, an approval, a stopped slice, a stall on disk,
    /// an unreadable plan or state, or an active feature whose daemon is gone.
    pub needs_you: bool,
    pub cost_usd: Option<f64>,
    pub cost_estimated: bool,
    /// When the feature was created (`initialized`).
    pub created_at: Option<u64>,
    /// The first start (first `started` event or attempt).
    pub started_at: Option<u64>,
    /// Every slice merged or skipped: when that happened.
    pub finished_at: Option<u64>,
    /// The last progress (settings edits and refused requests do not count).
    pub updated_at: Option<u64>,
    /// The plan or state cannot be read (the rest of the row is what could be).
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeatureDetail {
    #[serde(flatten)]
    pub row: FeatureRow,
    pub dir: String,
    pub max_parallel: u32,
    /// Seconds between zsf's polls (`limits.poll_secs`): how soon it notices a finished run.
    pub poll_secs: u64,
    pub branch_commit: Option<String>,
    /// `start`, `stop`, `unpause`, `max_parallel`: exactly what zsf accepts now.
    #[ts(type = "FeatureAction[]")]
    pub actions: Vec<&'static str>,
    pub slices: Vec<SliceView>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SliceView {
    pub id: String,
    pub title: String,
    /// zsf's status (`waiting_for_review`: a checkpoint's run is done, not merged), or
    /// `waiting_for_approval` for a checkpoint an older zsf merged before its review.
    #[ts(type = "SliceStatus")]
    pub status: &'static str,
    pub deps: Vec<String>,
    /// Pending only: deps not done yet.
    pub waits_for: Vec<String>,
    /// Of `waits_for`: checkpoints waiting for a person's review.
    pub approval_of: Vec<String>,
    pub checkpoint: bool,
    pub approved: bool,
    pub accepted: bool,
    pub reason: Option<String>,
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub worktree_exists: bool,
    /// A person's notes, added to every later task.
    pub notes: Vec<String>,
    /// zsf's own notes (the previous work after a conflict, the gate's output after it failed).
    pub auto_notes: Vec<AutoNoteView>,
    /// A retry waits to launch (paused, stopped or every slot busy).
    pub retry_queued: bool,
    /// Pending with every dependency done, but not launching: why (`waits for lease e2e-box`,
    /// `held while paused`, `2 of 2 slots busy`, `waits for Start`); absent = about to launch.
    pub hold: Option<String>,
    /// What a stopped slice's worktree holds against the feature branch (Accept merges this), or
    /// a slice waiting for review (Approve checks and merges this).
    pub worktree_changes: Option<ChangeStat>,
    /// The pull request the slice's run delivered, when it recorded one (`PR.md` among the
    /// feature's declared artifacts: in the worktree, else saved by zsf). Detail only.
    pub pr_url: Option<String>,
    pub leases: Vec<String>,
    /// When a person approved / accepted / skipped it (from the events).
    pub approved_at: Option<u64>,
    pub skipped_at: Option<u64>,
    pub merged_at: Option<u64>,
    pub spec: Option<String>,
    pub spec_path: String,
    pub attempts: Vec<AttemptView>,
    pub artifacts: Vec<String>,
    pub duration_ms: Option<u64>,
    pub cost_usd: Option<f64>,
    pub cost_estimated: bool,
    /// `approve`, `retry`, `accept`, `skip`, `edit`: exactly what zsf accepts now.
    #[ts(type = "SliceAction[]")]
    pub actions: Vec<&'static str>,
}

/// A note zsf added to every later task, and why: `conflict` (the earlier work, carried onto the
/// new branch tip) or `gate_failed` (the merge gate's output).
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutoNoteView {
    #[ts(type = "'conflict' | 'gate_failed'")]
    pub cause: AutoNoteCause,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    pub n: u32,
    #[ts(type = "'first' | 'infra-retry' | 'rerun' | 'manual-retry' | 'gate-rerun'")]
    pub kind: AttemptKind,
    pub fresh: bool,
    pub run_id: Option<String>,
    /// Where the run can be opened in zerochat.
    pub run_target: Option<String>,
    pub run_status: Option<String>,
    /// Runs this attempt resumed from (oldest first), each with its target when readable.
    pub earlier_runs: Vec<RunRef>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    #[ts(type = "AttemptResult | null")]
    pub result: Option<AttemptResult>,
    pub reason: Option<String>,
    /// A conflict kept the attempt's work on this branch.
    pub backup: Option<String>,
    /// Time its Zeroshot run(s) took (each run's own start to end); absent while one runs.
    pub run_ms: Option<u64>,
    /// The current run has ended (while the slice may still say running: nobody merged it yet).
    pub run_ended: bool,
    /// When the current run's agents finished, while Zeroshot is still finishing the run (its
    /// postmortem): zsf takes the slice up (the checks before merge) only once Zeroshot is done.
    pub run_closing_since: Option<u64>,
    /// When Zeroshot wholly finished the current run (its last event, after the postmortem):
    /// zsf takes the slice up from then, on its next poll. `None` while it runs or closes.
    pub run_finished_at: Option<u64>,
    pub has_task: bool,
    pub cost_usd: Option<f64>,
    pub cost_estimated: bool,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunRef {
    pub run_id: String,
    pub target: Option<String>,
}

/// Files, additions and deletions.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangeStat {
    pub files: Vec<FileStat>,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileStat {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub attempt: u32,
    pub text: String,
    pub previous_attempt: Option<u32>,
    /// Unified diff of the previous attempt's task to this one.
    pub diff: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommitView {
    pub commit: String,
    pub message: String,
    pub diff: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    #[serde(flatten)]
    pub event: Event,
    pub run_target: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDiff {
    pub fork: String,
    pub diff: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeatureCommit {
    pub sha: String,
    pub subject: String,
    pub slice: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeatureChanges {
    pub base: String,
    pub branch: String,
    pub head: String,
    pub commits: Vec<FeatureCommit>,
    pub stat: ChangeStat,
    pub diff: Option<String>,
}
