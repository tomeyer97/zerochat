//! zsf's plan, state and events projected into the views: one row per feature, one view per
//! slice and per attempt. The rules of what to offer come from `zsf::actions`; the rules here are
//! zerochat's own (what needs a person, why a ready slice is held back).

use std::path::{Path, PathBuf};

use zsf::model::{
    Attempt, Event, EventKind, FeatureConfig, Plan, SlicePlan, SliceState, SliceStatus, State,
};
use zsf::store::Feature;
use zsf::{FeatureAction, FeatureFacts, SliceAction};

use super::Ctx;
use super::files::{MAX_SPEC_BYTES, artifacts, cap, pr_url, task_file};
use super::git::{DiffOutput, parse_numstat, worktree_against_branch};
use super::runs::RunFacts;
use super::view::{AttemptView, AutoNoteView, ChangeStat, FeatureRow, RunRef, SliceView};

/// A feature as read from disk: what could be read, and why the rest could not.
pub(super) struct Loaded {
    pub feature: Feature,
    pub plan: Option<Plan>,
    pub state: State,
    pub events: Vec<Event>,
    /// The plan or state cannot be read (their errors, joined).
    pub issue: Option<String>,
}

impl Loaded {
    pub fn read(feature: Feature) -> Loaded {
        let mut issues = Vec::new();
        let plan = feature
            .load_plan()
            .map_err(|e| issues.push(format!("{e:#}")))
            .ok();
        let state = feature
            .load_state()
            .map_err(|e| issues.push(format!("{e:#}")))
            .unwrap_or_default();
        let events = feature.events();
        Loaded {
            feature,
            plan,
            state,
            events,
            issue: (!issues.is_empty()).then(|| issues.join("; ")),
        }
    }
}

// ---------------------------------------------------------------- actions offered

/// The slice actions zsf offers now, by name (`zsf::actions::slice_actions`; `accept` only when
/// the worktree holds changes, there is nothing to merge otherwise).
pub fn slice_actions(
    plan: &SlicePlan,
    s: &SliceState,
    worktree_changed: bool,
) -> Vec<&'static str> {
    zsf::actions::slice_actions(plan, s, worktree_changed)
        .into_iter()
        .map(SliceAction::as_str)
        .collect()
}

/// The feature actions zsf offers now, by name (`start` needs a `zsf` program to run the
/// daemon); nothing while the plan or state cannot be read. Only the goldens' action table calls
/// this positional form; zerochat itself uses [`offered_feature_actions`].
#[cfg(test)]
pub fn feature_actions(
    state: &State,
    all_finished: bool,
    daemon_alive: bool,
    can_spawn: bool,
    readable: bool,
) -> Vec<&'static str> {
    offered_feature_actions(
        state,
        FeatureFacts {
            all_finished,
            daemon_alive,
            can_spawn,
        },
        readable,
    )
    .into_iter()
    .map(FeatureAction::as_str)
    .collect()
}

pub(super) fn offered_feature_actions(
    state: &State,
    facts: FeatureFacts,
    readable: bool,
) -> Vec<FeatureAction> {
    if readable {
        zsf::actions::feature_actions(state, facts)
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------- zsf's notes

// zsf's `State::note` is a sentence for people; these are the openings zerochat tells apart.
// zsf writes them in its engine (`Engine::waiting_note` and the disk check in `schedule`): keep
// them in step with it.

/// `waiting for you: review 02, fix 04`: the reviews and failures, which the row lists already.
const NOTE_WAITING_FOR_YOU: &str = "waiting for you";
/// Nothing is ready and nothing waits for a person.
const NOTE_NOTHING_CAN_START: &str = "nothing can start";
/// `disk below 25 GB (12 GB free) before 04`: launching stalls until there is room.
const NOTE_DISK_BELOW: &str = "disk below";

/// zsf's note when it says something the other fields do not (a stall on disk or a lease).
fn stall_note(state: &State) -> Option<String> {
    state
        .note
        .clone()
        .filter(|n| !n.starts_with(NOTE_WAITING_FOR_YOU) && n != NOTE_NOTHING_CAN_START)
}

/// Why a pending slice whose dependencies are done is not launching (`None`: it is about to).
fn hold(
    plan: &Plan,
    state: &State,
    config: &FeatureConfig,
    p: &SlicePlan,
    daemon_alive: bool,
) -> Option<String> {
    if state.paused.is_some() {
        return Some("held while paused".to_owned());
    }
    if !state.active {
        return Some("waits for Start".to_owned());
    }
    if !daemon_alive {
        return Some("waits for the daemon".to_owned());
    }
    for lease in &p.leases {
        let cap = config.leases.get(lease).copied().unwrap_or(0) as usize;
        let held = plan
            .slices
            .iter()
            .filter(|q| {
                q.leases.contains(lease)
                    && state
                        .slices
                        .get(&q.id)
                        .is_some_and(|s| s.status.is_active())
            })
            .count();
        if held >= cap {
            return Some(format!("waits for lease {lease} ({held} of {cap} in use)"));
        }
    }
    // The disk note names the slice it stalled before; the hold is the note without that name.
    let before_this = format!(" before {}", p.id);
    if let Some(note) = &state.note
        && note.starts_with(NOTE_DISK_BELOW)
        && note.ends_with(&before_this)
    {
        return Some(note.trim_end_matches(&before_this).to_owned());
    }
    let busy = state.active_count();
    let max = config.limits.max_parallel.max(1) as usize;
    (busy >= max).then(|| format!("{busy} of {max} slots busy"))
}

// ---------------------------------------------------------------- feature rows

/// Checkpoints waiting for review, and stopped slices: what the feature waits for from a person.
pub fn waiting_for_you(plan: &Plan, state: &State) -> (Vec<String>, Vec<String>) {
    let (mut approve, mut failed) = (Vec::new(), Vec::new());
    for p in &plan.slices {
        let Some(s) = state.slices.get(&p.id) else {
            continue;
        };
        if s.awaiting_approval(p.checkpoint) {
            approve.push(p.id.clone());
        } else if s.status == SliceStatus::Failed {
            failed.push(p.id.clone());
        }
    }
    (approve, failed)
}

fn status_of(state: &State, id: &str) -> SliceStatus {
    state.slices.get(id).map(|s| s.status).unwrap_or_default()
}

/// Where a feature's slices stand: what the row shows and the "needs you" rule reads.
struct Progress {
    /// Every slice finished (a merged checkpoint counts while it waits for approval, as in `act`).
    all_finished: bool,
    approve: Vec<String>,
    failed: Vec<String>,
    /// Slices launching, running or integrating.
    running: Vec<String>,
    /// [`stall_note`], while not finished.
    note: Option<String>,
    daemon_pid: Option<u32>,
}

impl Progress {
    fn of(loaded: &Loaded, plan: &Plan) -> Progress {
        let state = &loaded.state;
        let all_finished = !plan.slices.is_empty() && state.all_finished(plan);
        let (approve, failed) = waiting_for_you(plan, state);
        let running = plan
            .slices
            .iter()
            .filter(|p| status_of(state, &p.id).is_active())
            .map(|p| p.id.clone())
            .collect();
        let note = if all_finished {
            None
        } else {
            stall_note(state)
        };
        Progress {
            all_finished,
            approve,
            failed,
            running,
            note,
            daemon_pid: loaded.feature.daemon_pid(),
        }
    }
}

/// Something only a person can unblock: an unreadable plan or state, a pause, an approval, a
/// stopped slice, a stall on disk, a stall with nothing running, or an active feature whose
/// daemon is gone.
fn needs_person(loaded: &Loaded, progress: &Progress) -> bool {
    let state = &loaded.state;
    let note = progress.note.as_deref();
    let stalled_on_disk = note.is_some_and(|n| n.starts_with(NOTE_DISK_BELOW));
    let stalled = note.is_some() && progress.running.is_empty();
    let daemon_gone = state.active && progress.daemon_pid.is_none() && !progress.all_finished;
    loaded.issue.is_some()
        || state.paused.is_some()
        || !progress.approve.is_empty()
        || !progress.failed.is_empty()
        || stalled_on_disk
        || stalled
        || daemon_gone
}

/// [`needs_person`] for a feature, without reading its runs.
pub(super) fn feature_needs_person(loaded: &Loaded) -> bool {
    let empty = Plan::default();
    let plan = loaded.plan.as_ref().unwrap_or(&empty);
    needs_person(loaded, &Progress::of(loaded, plan))
}

pub(super) fn last_event(events: &[Event], kind: EventKind, slice: Option<&str>) -> Option<u64> {
    events
        .iter()
        .rev()
        .find(|e| e.kind == kind && (slice.is_none() || e.slice.as_deref() == slice))
        .map(|e| e.ts)
}

/// The first start: the first `started` event or the first attempt, whichever came first.
fn first_start(state: &State, events: &[Event]) -> Option<u64> {
    let first_attempt = state
        .slices
        .values()
        .flat_map(|s| s.attempts.iter().map(|a| a.started_ms))
        .min();
    let first_started = events
        .iter()
        .find(|e| e.kind == EventKind::Started)
        .map(|e| e.ts);
    [first_attempt, first_started].into_iter().flatten().min()
}

/// The last progress. Settings edits, refused requests and the daemon's own coming and going
/// are not progress.
fn last_progress(events: &[Event]) -> Option<u64> {
    let is_progress = |e: &Event| {
        let bookkeeping = matches!(
            e.kind,
            EventKind::RequestRejected | EventKind::DaemonStarted | EventKind::DaemonStopped
        );
        let settings_edit = e.kind == EventKind::Edited && e.slice.is_none();
        !(bookkeeping || settings_edit)
    };
    events.iter().rev().find(|e| is_progress(e)).map(|e| e.ts)
}

/// When a finished feature finished: its `done` event, else its last attempt's end, else its
/// last progress.
fn finished_at(state: &State, events: &[Event], last_progress: Option<u64>) -> Option<u64> {
    let last_attempt_end = || {
        state
            .slices
            .values()
            .flat_map(|s| s.attempts.iter().filter_map(|a| a.ended_ms))
            .max()
    };
    last_event(events, EventKind::Done, None)
        .or_else(last_attempt_end)
        .or(last_progress)
}

/// What a slice or feature spent: the sum of the costs that are known, estimated when any of
/// them is.
fn sum_costs(costs: impl Iterator<Item = (Option<f64>, bool)>) -> (Option<f64>, bool) {
    let mut total = None;
    let mut estimated = false;
    for (cost, est) in costs {
        if let Some(c) = cost {
            *total.get_or_insert(0.0) += c;
            estimated |= est;
        }
    }
    (total, estimated)
}

/// The feature's row; `slices` (its slice views) give its cost.
pub(super) fn row(loaded: &Loaded, slices: &[SliceView]) -> FeatureRow {
    let empty = Plan::default();
    let plan = loaded.plan.as_ref().unwrap_or(&empty);
    let (feature, state, events) = (&loaded.feature, &loaded.state, &loaded.events);
    let progress = Progress::of(loaded, plan);
    let needs_you = needs_person(loaded, &progress);
    let started_at = first_start(state, events);
    let updated = last_progress(events);
    let finished_at = if progress.all_finished {
        finished_at(state, events, updated)
    } else {
        None
    };
    let (cost_usd, cost_estimated) =
        sum_costs(slices.iter().map(|s| (s.cost_usd, s.cost_estimated)));
    let count = |status: SliceStatus| {
        plan.slices
            .iter()
            .filter(|p| status_of(state, &p.id) == status)
            .count()
    };
    let Progress {
        approve,
        failed,
        running,
        note,
        daemon_pid,
        ..
    } = progress;
    FeatureRow {
        name: feature.config.name.clone(),
        repo: feature.resolve(&feature.config.repo).display().to_string(),
        branch: feature.config.branch.clone(),
        base: feature.config.base.clone(),
        merged: count(SliceStatus::Merged),
        skipped: count(SliceStatus::Skipped),
        total: plan.slices.len(),
        running,
        approve,
        failed,
        paused: state.paused.clone(),
        paused_at: state
            .paused
            .as_ref()
            .and_then(|_| last_event(events, EventKind::Paused, None)),
        note,
        active: state.active,
        daemon_pid,
        started: started_at.is_some(),
        needs_you,
        cost_usd,
        cost_estimated,
        created_at: events.first().map(|e| e.ts),
        started_at,
        finished_at,
        updated_at: finished_at.or(updated),
        issue: loaded.issue.clone(),
    }
}

// ---------------------------------------------------------------- slice and attempt views

/// The view of every slice in the plan (none while the plan cannot be read); `with_text` adds
/// the spec and the artifact list (the detail page has them, the list does not).
pub(super) fn slice_views(ctx: &Ctx, loaded: &Loaded, with_text: bool) -> Vec<SliceView> {
    let Some(plan) = &loaded.plan else {
        return Vec::new();
    };
    let daemon_alive = loaded.feature.daemon_pid().is_some();
    plan.slices
        .iter()
        .map(|p| slice_view(ctx, loaded, plan, p, daemon_alive, with_text))
        .collect()
}

fn slice_view(
    ctx: &Ctx,
    loaded: &Loaded,
    plan: &Plan,
    p: &SlicePlan,
    daemon_alive: bool,
    with_text: bool,
) -> SliceView {
    let (feature, state) = (&loaded.feature, &loaded.state);
    let s = state.slices.get(&p.id).cloned().unwrap_or_default();
    let attempts: Vec<AttemptView> = s
        .attempts
        .iter()
        .map(|a| attempt_view(ctx, feature, &p.id, a))
        .collect();
    let duration_ms = slice_duration(&attempts, ctx.now);
    let (cost_usd, cost_estimated) =
        sum_costs(attempts.iter().map(|a| (a.cost_usd, a.cost_estimated)));
    let waits_for = state.waits_for(plan, p);
    let approval_of = waits_for
        .iter()
        .filter(|d| state.awaiting_approval(plan, d))
        .cloned()
        .collect();
    let ready = s.status == SliceStatus::Pending
        && s.job.is_none()
        && waits_for.is_empty()
        && loaded.issue.is_none();
    let hold = if ready {
        hold(plan, state, &feature.config, p, daemon_alive)
    } else {
        None
    };
    let worktree_changes = worktree_changes(&feature.config, &s);
    let changed = worktree_changes
        .as_ref()
        .is_some_and(|c| !c.files.is_empty());
    let spec_path = feature.spec_path(&p.spec);
    let event_at = |kind: EventKind| last_event(&loaded.events, kind, Some(&p.id));
    let merged_at = || event_at(EventKind::Merged).or_else(|| event_at(EventKind::Accepted));
    SliceView {
        id: p.id.clone(),
        title: p.title.clone(),
        status: s.display_status(p.checkpoint),
        deps: p.deps.clone(),
        waits_for,
        approval_of,
        checkpoint: p.checkpoint,
        approved: s.approved,
        accepted: s.accepted,
        reason: s.reason.clone(),
        commit: s.commit.clone(),
        branch: s.branch.clone(),
        worktree_exists: s.worktree_on_disk().is_some(),
        worktree: s.worktree.as_ref().map(|w| w.display().to_string()),
        notes: s.notes.clone(),
        auto_notes: s
            .auto_notes
            .iter()
            .map(|n| AutoNoteView {
                cause: n.cause,
                text: n.text.clone(),
            })
            .collect(),
        retry_queued: s.retry.is_some(),
        hold,
        worktree_changes,
        pr_url: if with_text {
            pr_url(feature, &p.id, s.worktree_on_disk().map(PathBuf::as_path))
        } else {
            None
        },
        leases: p.leases.clone(),
        approved_at: if s.approved {
            event_at(EventKind::Approved)
        } else {
            None
        },
        skipped_at: if s.status == SliceStatus::Skipped {
            event_at(EventKind::Skipped)
        } else {
            None
        },
        merged_at: if s.status == SliceStatus::Merged {
            merged_at()
        } else {
            None
        },
        spec: if with_text {
            read_spec(&spec_path)
        } else {
            None
        },
        spec_path: spec_path.display().to_string(),
        attempts,
        artifacts: if with_text {
            artifacts(feature, &p.id)
        } else {
            Vec::new()
        },
        duration_ms,
        cost_usd,
        cost_estimated,
        actions: if loaded.issue.is_some() {
            Vec::new()
        } else {
            slice_actions(p, &s, changed)
        },
    }
}

fn read_spec(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| cap(text, MAX_SPEC_BYTES))
}

/// The time a slice's attempts took. A finished run counts its own time, not the time zsf took
/// to notice it (a stopped daemon); an attempt without one counts until it ended (or now).
fn slice_duration(attempts: &[AttemptView], now: u64) -> Option<u64> {
    if attempts.is_empty() {
        return None;
    }
    let attempt_ms = |a: &AttemptView| {
        a.run_ms
            .unwrap_or_else(|| a.ended_at.unwrap_or(now).saturating_sub(a.started_at))
    };
    Some(attempts.iter().map(attempt_ms).sum())
}

fn attempt_view(ctx: &Ctx, feature: &Feature, slice: &str, a: &Attempt) -> AttemptView {
    let facts = a
        .run_id
        .as_deref()
        .and_then(|run| ctx.runs.get(ctx.roots, run, ctx.now));
    let earlier: Vec<(RunRef, Option<RunFacts>)> = a
        .earlier_runs
        .iter()
        .map(|run| earlier_run(ctx, run))
        .collect();
    // The current run and the runs it resumed from, those that can be read.
    let runs: Vec<&RunFacts> = facts
        .iter()
        .chain(earlier.iter().filter_map(|(_, f)| f.as_ref()))
        .collect();
    let (cost_usd, cost_estimated) = sum_costs(runs.iter().map(|f| (f.cost_usd, f.cost_estimated)));
    let run_ended = facts.as_ref().is_some_and(|f| f.ended_at.is_some());
    let run_ms = if a.run_id.is_some() && run_ended {
        total_run_time(&runs)
    } else {
        None
    };
    AttemptView {
        n: a.n,
        kind: a.kind,
        fresh: a.fresh,
        run_id: a.run_id.clone(),
        run_target: facts.as_ref().map(|f| f.target.clone()),
        run_status: facts.as_ref().map(|f| f.status.clone()),
        earlier_runs: earlier.into_iter().map(|(run, _)| run).collect(),
        started_at: a.started_ms,
        ended_at: a.ended_ms,
        result: a.result,
        reason: a.reason.clone(),
        backup: a.backup.clone(),
        run_ms,
        run_ended,
        run_closing_since: facts
            .as_ref()
            .filter(|f| f.closing)
            .and_then(|f| f.ended_at),
        run_finished_at: facts.as_ref().filter(|f| !f.closing).and_then(|f| {
            f.ended_at
                .map(|end| end.max(f.last_activity_at.unwrap_or(end)))
        }),
        has_task: task_file(feature, slice, a.n).is_file(),
        cost_usd,
        cost_estimated,
    }
}

/// A run an attempt resumed from, with its target when it can be read.
fn earlier_run(ctx: &Ctx, run: &str) -> (RunRef, Option<RunFacts>) {
    let facts = ctx.runs.get(ctx.roots, run, ctx.now);
    (
        RunRef {
            run_id: run.to_owned(),
            target: facts.as_ref().map(|f| f.target.clone()),
        },
        facts,
    )
}

/// Each run's own start to end, summed; `None` when one lacks either.
fn total_run_time(runs: &[&RunFacts]) -> Option<u64> {
    runs.iter().try_fold(0u64, |sum, f| {
        Some(sum + f.ended_at?.saturating_sub(f.started_at?))
    })
}

/// What the worktree of a stopped slice (what Accept would merge) or of a slice waiting for review
/// (what Approve checks and merges) holds against the feature branch.
pub(super) fn worktree_changes(config: &FeatureConfig, s: &SliceState) -> Option<ChangeStat> {
    if !s.is_stopped() && !s.waiting_for_review() {
        return None;
    }
    let worktree = s.worktree_on_disk()?;
    let (_, numstat) =
        worktree_against_branch(worktree, &config.branch, DiffOutput::Numstat).ok()?;
    Some(parse_numstat(&numstat))
}
