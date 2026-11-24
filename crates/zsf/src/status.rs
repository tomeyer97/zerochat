//! `zsf status`: one report from the plan, the state and the daemon, for people and for zerochat.

use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use crate::model::{Attempt, DisplayStatus, Event, Plan, State};
use crate::store::Feature;

#[derive(Clone, Debug, Serialize)]
pub struct FeatureReport {
    pub name: String,
    pub dir: PathBuf,
    pub repo: PathBuf,
    pub branch: String,
    pub base: String,
    /// The feature branch's commit, when the repository can be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_commit: Option<String>,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The daemon's pid when it runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon_pid: Option<u32>,
    pub max_parallel: u32,
    pub merged: usize,
    pub total: usize,
    pub slices: Vec<SliceReport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SliceReport {
    pub id: String,
    pub title: String,
    /// The state's status (`waiting_for_review` for a checkpoint whose run is done, not merged),
    /// or `waiting_for_approval` for a checkpoint an older zsf merged before its review.
    pub status: DisplayStatus,
    pub deps: Vec<String>,
    /// Deps that are not done yet (pending slices only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub waits_for: Vec<String>,
    pub checkpoint: bool,
    pub approved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<Attempt>,
    pub attempts: usize,
}

pub fn report(feature: &Feature) -> Result<FeatureReport> {
    let plan = feature.load_plan()?;
    let state = feature.load_state()?;
    let repo = feature.resolve(&feature.config.repo);
    let branch_commit =
        crate::git::rev_parse(&repo, &format!("refs/heads/{}", feature.config.branch)).ok();
    Ok(build(feature, &plan, &state, branch_commit))
}

pub fn build(
    feature: &Feature,
    plan: &Plan,
    state: &State,
    branch_commit: Option<String>,
) -> FeatureReport {
    let slices: Vec<SliceReport> = plan
        .slices
        .iter()
        .map(|p| {
            let s = state.slices.get(&p.id).cloned().unwrap_or_default();
            SliceReport {
                id: p.id.clone(),
                title: p.title.clone(),
                status: s.display(p.checkpoint),
                deps: p.deps.clone(),
                waits_for: state.waits_for(plan, p),
                checkpoint: p.checkpoint,
                approved: s.approved,
                reason: s.reason.clone(),
                commit: s.commit.clone(),
                worktree: s.worktree.clone(),
                attempt: s.attempts.last().cloned(),
                attempts: s.attempts.len(),
            }
        })
        .collect();
    FeatureReport {
        name: feature.config.name.clone(),
        dir: feature.dir.clone(),
        repo: feature.resolve(&feature.config.repo),
        branch: feature.config.branch.clone(),
        base: feature.config.base.clone(),
        branch_commit,
        active: state.active,
        paused: state.paused.clone(),
        note: state.note.clone(),
        daemon_pid: feature.daemon_pid(),
        max_parallel: feature.config.limits.max_parallel,
        merged: slices
            .iter()
            .filter(|s| {
                matches!(
                    s.status,
                    DisplayStatus::Merged | DisplayStatus::WaitingForApproval
                )
            })
            .count(),
        total: slices.len(),
        slices,
    }
}

/// Characters of a commit id shown in the human view.
const SHORT_SHA: usize = 10;
/// The title column is as wide as the longest title, up to this many characters.
const TITLE_WIDTH: usize = 44;
/// Characters kept from the end of a long failure reason (the end of hook output says most).
const REASON_TAIL: usize = 140;
/// The per-slice detail (and each line of it) is cut after this many characters.
const EXTRA_WIDTH: usize = 160;
/// Width of the status column (`{:<20}`).
const STATUS_WIDTH: usize = 20;
/// The columns of a slice line other than the title, up to the detail: two spaces, a two-character
/// id, the checkpoint mark, the status column and the separators (2 + 2 + 1 + 1 + 20 + 1 + 2).
/// The worktree line under a failed (or waiting) slice is indented by this plus the title width, so it lines
/// up with the detail.
const DETAIL_INDENT: usize = 29;

/// The human view: a header line and one line per slice.
pub fn render(report: &FeatureReport) -> String {
    let now = crate::time::now_ms();
    let mut out = String::new();
    let commit = report
        .branch_commit
        .as_deref()
        .map(|c| format!(" at {}", short_sha(c)))
        .unwrap_or_default();
    let daemon = match report.daemon_pid {
        Some(pid) => format!("daemon running (pid {pid})"),
        None => "daemon not running".to_owned(),
    };
    out.push_str(&format!(
        "{}: {} of {} merged, branch {}{commit}, {daemon}, {}\n",
        report.name,
        report.merged,
        report.total,
        report.branch,
        if report.active { "active" } else { "inactive" }
    ));
    if let Some(paused) = &report.paused {
        out.push_str(&format!(
            "PAUSED: {paused}  (zsf unpause {})\n",
            report.name
        ));
    }
    if let Some(note) = &report.note {
        out.push_str(&format!("{note}\n"));
    }
    let width = report
        .slices
        .iter()
        .map(|s| s.title.chars().count())
        .max()
        .unwrap_or(0)
        .min(TITLE_WIDTH);
    for s in &report.slices {
        let title: String = s.title.chars().take(width).collect();
        let extra = slice_extra(report, s, width, now);
        let mark = if s.checkpoint { "*" } else { " " };
        out.push_str(&format!(
            "  {}{mark} {:<STATUS_WIDTH$} {:<width$}  {}\n",
            s.id,
            s.status.as_str(),
            title,
            head(&extra, EXTRA_WIDTH)
        ));
    }
    out
}

/// The detail after a slice's title: what it is doing, where it merged, or why it stopped.
fn slice_extra(report: &FeatureReport, s: &SliceReport, width: usize, now: u64) -> String {
    let commit = s.commit.as_deref().map(short_sha).unwrap_or("");
    match s.status {
        DisplayStatus::Running | DisplayStatus::Launching | DisplayStatus::Integrating => s
            .attempt
            .as_ref()
            .map(|a| {
                format!(
                    "{} attempt {}, run {}, {}",
                    a.kind.as_str(),
                    a.n,
                    a.run_id.as_deref().unwrap_or("-"),
                    crate::time::duration(now.saturating_sub(a.started_ms))
                )
            })
            .unwrap_or_default(),
        DisplayStatus::Merged => commit.to_owned(),
        DisplayStatus::WaitingForApproval => {
            format!("{commit}  approve: zsf approve {} {}", report.name, s.id)
        }
        DisplayStatus::WaitingForReview => format!(
            "not merged; check and merge: zsf approve {} {}{}",
            report.name,
            s.id,
            worktree_line(s, width)
        ),
        DisplayStatus::Failed | DisplayStatus::Interrupted => {
            // The worktree goes on the next line, under the reason.
            let reason = tail_chars(&one_line(s.reason.as_deref().unwrap_or("")), REASON_TAIL);
            format!("{reason}{}", worktree_line(s, width))
        }
        DisplayStatus::Pending if !s.waits_for.is_empty() => {
            format!("waits for {}", s.waits_for.join(", "))
        }
        DisplayStatus::Pending => "ready".to_owned(),
        _ => String::new(),
    }
}

/// The slice's worktree on a line of its own, under the detail (empty without one).
fn worktree_line(s: &SliceReport, width: usize) -> String {
    s.worktree
        .as_ref()
        .map(|w| {
            let indent = width + DETAIL_INDENT;
            format!("\n{:>indent$}worktree {}", "", w.display())
        })
        .unwrap_or_default()
}

fn short_sha(commit: &str) -> &str {
    &commit[..commit.len().min(SHORT_SHA)]
}

/// The last `max` characters of `text`, after `…` when some were cut.
fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count - max).collect();
    format!("…{tail}")
}

/// One `zsf log` line: time, slice and attempt, event kind, run, and the detail on one line.
pub fn log_line(e: &Event) -> String {
    let slice = match (&e.slice, e.attempt) {
        (Some(s), Some(a)) => format!("{s} a{a}"),
        (Some(s), None) => s.clone(),
        _ => "-".to_owned(),
    };
    let run = e
        .run_id
        .as_deref()
        .map(|r| format!(" run {r}"))
        .unwrap_or_default();
    let detail = if e.detail.is_empty() {
        String::new()
    } else {
        format!("  {}", one_line(&e.detail))
    };
    format!(
        "{}  {slice:<7} {}{run}{detail}",
        crate::time::utc(e.ts),
        e.kind.as_str()
    )
}

/// Several lines of output (a hook's) as one: blank lines and rules (`-----`) dropped, the rest
/// joined with ` / `.
pub fn one_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.chars().all(|c| matches!(c, '-' | '=' | '_' | '*')))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn head(text: &str, max: usize) -> String {
    if text.contains('\n') {
        let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
        return format!("{}\n{rest}", head(first, max));
    }
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_output_reads_as_one_line() {
        let out = "\n----------------------------------------------------------------------\nRan 4 tests in 0.000s\n\nOK\ngate: this slice must add NOTES.md\n";
        assert_eq!(
            one_line(out),
            "Ran 4 tests in 0.000s / OK / gate: this slice must add NOTES.md"
        );
        assert_eq!(one_line("gate failed: x"), "gate failed: x");
    }
}
