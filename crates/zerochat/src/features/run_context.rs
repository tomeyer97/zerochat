//! A run's place in a feature: which slice and attempt it ran, and what became of the attempt's
//! work. The run page's context line and the sidebar's label are both written from it.

use std::collections::HashMap;
use std::path::Path;

use zsf::model::{Attempt, AttemptKind, AttemptResult, Plan, SliceState, SliceStatus};
use zsf::store::Feature;

/// `run id → slice` across every feature (built once per provider).
pub struct SliceIndex {
    pub runs: HashMap<String, SliceRun>,
}

#[derive(Clone, Debug)]
pub struct SliceRun {
    pub feature: String,
    pub slice: String,
    pub title: String,
    pub attempt: u32,
    pub attempts: usize,
    /// What zsf did with the attempt's work, when it is decided.
    pub outcome: Option<Outcome>,
}

/// What became of an attempt's work.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The run's controller was lost; a later run of the same attempt took over.
    ResumedLater,
    /// Merged: through the gate, or accepted by a person without it (`gated: false`); the squash
    /// commit when this was the slice's last attempt.
    Merged { gated: bool, commit: Option<String> },
    /// Not merged (`why`: zsf's result in a few words), and the attempt that came next, if one did.
    NotMerged {
        why: String,
        next: Option<NextAttempt>,
    },
    /// Abandoned for attempt `by`.
    Replaced { by: u32 },
    /// The slice was skipped.
    SliceSkipped,
}

/// The attempt that followed one that was not merged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NextAttempt {
    pub n: u32,
    pub kind: AttemptKind,
}

/// A commit's short form, as git abbreviates it.
const SHORT_SHA_LEN: usize = 7;

fn short_sha(commit: &str) -> &str {
    &commit[..commit.len().min(SHORT_SHA_LEN)]
}

impl Outcome {
    /// What became of attempt `a` of slice `s`, as seen from `run` (one of the attempt's runs).
    fn of(s: &SliceState, a: &Attempt, run: &str) -> Option<Outcome> {
        if a.earlier_runs.iter().any(|r| r == run) {
            return Some(Outcome::ResumedLater);
        }
        let last = s.attempts.last().is_some_and(|l| l.n == a.n);
        let next = s
            .attempts
            .iter()
            .find(|b| b.n == a.n + 1)
            .map(|b| NextAttempt {
                n: b.n,
                kind: b.kind,
            });
        if last && s.status == SliceStatus::Merged && s.accepted {
            return Some(Outcome::Merged {
                gated: false,
                commit: s.commit.clone(),
            });
        }
        let not_merged = |why: &str| Outcome::NotMerged {
            why: why.to_owned(),
            next,
        };
        Some(match a.result? {
            AttemptResult::Merged => Outcome::Merged {
                gated: true,
                commit: s.commit.clone().filter(|_| last),
            },
            AttemptResult::GateFailed => not_merged("gate failed, not merged"),
            AttemptResult::Empty => not_merged("no changes"),
            AttemptResult::Conflict => not_merged(a.reason.as_deref().unwrap_or("conflict")),
            AttemptResult::QualityFailed => not_merged("review rounds ran out"),
            AttemptResult::Stopped => not_merged("stopped"),
            AttemptResult::InfraFailed | AttemptResult::LaunchFailed => {
                not_merged("infrastructure failure")
            }
            AttemptResult::IntegrationFailed => not_merged("merge failed"),
            AttemptResult::Abandoned => match next {
                Some(b) => Outcome::Replaced { by: b.n },
                None => Outcome::NotMerged {
                    why: "stopped".to_owned(),
                    next: None,
                },
            },
            AttemptResult::Skipped => Outcome::SliceSkipped,
            AttemptResult::ChangesRequested => not_merged("changes requested"),
        })
    }

    pub fn is_merged(&self) -> bool {
        matches!(self, Outcome::Merged { .. })
    }

    /// In a few words, for the run page: `gate failed, not merged, re-run as attempt 2`,
    /// `merged (abc1234)`.
    pub fn sentence(&self) -> String {
        match self {
            Outcome::ResumedLater => "controller lost, resumed in a later run".to_owned(),
            Outcome::Merged { gated, commit } => {
                let how = if *gated {
                    "merged"
                } else {
                    "merged without the gate"
                };
                let commit = commit
                    .as_deref()
                    .map(|c| format!(" ({})", short_sha(c)))
                    .unwrap_or_default();
                format!("{how}{commit}")
            }
            Outcome::NotMerged { why, next: None } => why.clone(),
            Outcome::NotMerged {
                why,
                next: Some(next),
            } => {
                let how = match next.kind {
                    AttemptKind::Rerun => "re-run",
                    AttemptKind::ManualRetry => "retried",
                    AttemptKind::InfraRetry => "retried automatically",
                    AttemptKind::GateRerun => "re-run automatically",
                    AttemptKind::First => "run again",
                };
                format!("{why}, {how} as attempt {}", next.n)
            }
            Outcome::Replaced { by } => format!("replaced by attempt {by}"),
            Outcome::SliceSkipped => "slice skipped, not merged".to_owned(),
        }
    }

    /// Fewer words, for the sidebar: `gate failed`, `controller lost`, `merged`.
    pub fn short(&self) -> String {
        match self {
            Outcome::ResumedLater => "controller lost".to_owned(),
            Outcome::Merged { gated: true, .. } => "merged".to_owned(),
            Outcome::Merged { gated: false, .. } => "merged without the gate".to_owned(),
            Outcome::NotMerged { why, .. } => first_clause(why).to_owned(),
            Outcome::Replaced { by } => format!("replaced by attempt {by}"),
            Outcome::SliceSkipped => "slice skipped".to_owned(),
        }
    }
}

/// A reason up to its first clause: before a comma, a parenthesis or ` in ` (`conflicts with 02
/// in src/a.rs` → `conflicts with 02`; `gate failed, not merged` → `gate failed`).
fn first_clause(text: &str) -> &str {
    let text = text.split(',').next().unwrap_or(text);
    let text = text.split(" (").next().unwrap_or(text);
    text.split(" in ").next().unwrap_or(text)
}

/// Every run of every attempt of the feature's slices, with its place in the feature.
fn slice_runs<'a>(
    feature: &'a Feature,
    plan: &'a Plan,
    id: &'a str,
    s: &'a SliceState,
) -> impl Iterator<Item = (String, SliceRun)> + 'a {
    let title = plan.get(id).map(|p| p.title.clone()).unwrap_or_default();
    s.attempts.iter().flat_map(move |a| {
        let title = title.clone();
        a.run_id
            .iter()
            .chain(a.earlier_runs.iter())
            .map(move |run| {
                let slice_run = SliceRun {
                    feature: feature.config.name.clone(),
                    slice: id.to_owned(),
                    title: title.clone(),
                    attempt: a.n,
                    attempts: s.attempts.len(),
                    outcome: Outcome::of(s, a, run),
                };
                (run.clone(), slice_run)
            })
    })
}

impl SliceIndex {
    pub fn build(root: &Path) -> SliceIndex {
        let mut runs = HashMap::new();
        for name in super::names(root) {
            let Ok(feature) = Feature::open(root, &name) else {
                continue;
            };
            let Ok(state) = feature.load_state() else {
                continue;
            };
            let plan = feature.load_plan().unwrap_or_default();
            for (id, s) in &state.slices {
                runs.extend(slice_runs(&feature, &plan, id, s));
            }
        }
        SliceIndex { runs }
    }
}

/// "Slice 03 of calc-v2 · attempt 1 of 2 · gate failed, not merged, re-run as attempt 2".
pub fn context_line(run: &SliceRun) -> String {
    let of = if run.attempts > 1 {
        format!(" of {}", run.attempts)
    } else {
        String::new()
    };
    let outcome = run
        .outcome
        .as_ref()
        .map(|o| format!(" · {}", o.sentence()))
        .unwrap_or_default();
    format!(
        "Slice {} of {} · attempt {}{of}{outcome}",
        run.slice, run.feature, run.attempt
    )
}

/// The sidebar label of a slice run: its title, then its attempt and what became of it when
/// there were several attempts or this one was not merged.
pub fn context_label(run: &SliceRun) -> String {
    let mut label = format!("{} {}: {}", run.feature, run.slice, run.title);
    if run.attempts > 1 || run.outcome.as_ref().is_some_and(|o| !o.is_merged()) {
        label.push_str(&format!(" · #{}", run.attempt));
        if let Some(outcome) = &run.outcome {
            label.push_str(&format!(" {}", outcome.short()));
        }
    }
    label
}
