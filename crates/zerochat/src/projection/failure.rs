//! Failure classification: why a run failed, in words a person can act on (`classify`), and
//! what a provider's own error text says went wrong (`diagnose`).
//!
//! `classify` works in phases: find the execution that decided the outcome (`culprit`), gather
//! its error lines (`last_errors`), date the failure (`when`), explain it (explain.rs, with the
//! wording for agent failures in agent_copy.rs), then list what was kept and what can be done.
//! The parts:
//! - explain.rs: the reason Zeroshot closed the run → class, cause, title, fix, primary action;
//! - agent_copy.rs: the wording for a failed agent, one arm per cause;
//! - diagnose.rs: the provider error text → a cause (`diagnose`, a table of needles);
//! - http_status.rs: HTTP status codes in provider error text;
//! - reset.rs: when a usage limit resets.

mod agent_copy;
mod diagnose;
mod explain;
mod http_status;
mod reset;

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::graph::GraphNode;
use super::model::{Action, ExecState, Execution, Failure, LogLine};
use crate::source::LocalFacts;
use explain::{Case, Class, explain};

// What the projection's tests and goldens call on their own.
#[cfg(test)]
pub use diagnose::{Diagnosis, DiagnosisKind, diagnose};
#[cfg(test)]
pub use http_status::http_statuses;
#[cfg(test)]
pub use reset::parse_reset;

/// Everything `classify` needs, borrowed from the projection once every event is folded.
pub(super) struct Facts<'a> {
    pub(super) runtime: &'a Value,
    pub(super) phase: &'a str,
    pub(super) status: &'a str,
    pub(super) reason: &'a str,
    pub(super) terminal: &'a Value,
    pub(super) executions: &'a [Execution],
    pub(super) nodes: &'a HashMap<&'a str, &'a GraphNode>,
    pub(super) started_at: Option<u64>,
    pub(super) ended_at: Option<u64>,
    pub(super) run_log: &'a [LogLine],
    pub(super) local: Option<&'a LocalFacts>,
    pub(super) lost_before_resume: bool,
    pub(super) postmortem: bool,
    pub(super) now_ms: u64,
}

/// Why the run failed, or `None` when it did not (it is not failed, stalled or stopped).
pub(super) fn classify(f: &Facts) -> Option<Failure> {
    if !matches!(f.status, "failed" | "stalled" | "stopped") {
        return None;
    }
    let stalled = f.status == "stalled";
    let reason = match f.reason {
        _ if stalled => "controller_not_running",
        "" => "unknown",
        reason => reason,
    };
    let culprit = culprit(f.executions);
    let info = culprit.and_then(|e| f.nodes.get(e.node.as_str()).copied());
    let last_errors = last_errors(f, culprit);
    let when = when(f, culprit);
    let harness = culprit.and_then(|e| e.harness.as_deref());
    let provider = culprit.and_then(|e| e.provider.as_deref());

    let explanation = explain(&Case {
        f,
        reason,
        culprit,
        info,
        who: who(culprit, info),
        harness,
        company: Company::of(provider),
        last_errors: &last_errors,
        at: when.at,
    });

    let kept = if explanation.class == Class::Work {
        Vec::new()
    } else {
        kept(f.executions, culprit)
    };
    let actions = actions(f, explanation.resume_helps);
    // A run that came from a resume or replay of another already had its unchanged second try:
    // another unchanged replay is not the way forward (it stays a secondary action).
    let second_try = f.local.is_some_and(|l| l.resumed_from.is_some());
    let primary = explanation
        .primary
        .filter(|p| actions.contains(&p.base()))
        .filter(|p| !(second_try && p.base() == Action::Replay));
    Some(Failure {
        class: explanation.class.as_str(),
        cause: explanation.cause,
        title: explanation.title,
        detail: explanation.detail,
        fix: explanation.fix,
        fix_command: explanation.fix_command,
        exit: fail_node(f, reason).map(|n| n.name.clone()),
        reason: reason.to_owned(),
        node: culprit.map(|e| e.node.clone()),
        node_kind: info.map(|n| n.kind.clone()),
        execution: culprit.map(|e| e.id),
        round: culprit.map(|e| e.round),
        attempt: culprit.map(|e| e.attempt),
        at: when.at,
        into_run_ms: match (f.started_at, when.at) {
            (Some(s), Some(a)) => Some(a.saturating_sub(s)),
            _ => None,
        },
        closed_after_ms: when.closed_after_ms,
        closed_by_postmortem: when.closed_after_ms.is_some() && f.postmortem,
        kept,
        feedback: explanation.feedback,
        resets_at: explanation.resets_at,
        last_errors,
        primary,
        actions,
    })
}

/// The execution that decided the outcome: the last failed or interrupted one, else the last
/// rejected one, else the last.
fn culprit(executions: &[Execution]) -> Option<&Execution> {
    executions
        .iter()
        .rev()
        .find(|e| matches!(e.state, ExecState::Failed | ExecState::Interrupted))
        .or_else(|| {
            executions
                .iter()
                .rev()
                .find(|e| e.state == ExecState::Rejected)
        })
        .or_else(|| executions.last())
}

/// The error lines to show: the culprit's, plus what Zeroshot recorded when it closed the run
/// (else the run log's last five errors), each provider sentence once.
fn last_errors(f: &Facts, culprit: Option<&Execution>) -> Vec<String> {
    let mut lines: Vec<String> = culprit.map(|e| e.error_lines.clone()).unwrap_or_default();
    let recorded = f
        .terminal
        .pointer("/context/recorded")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for message in recorded.filter_map(|r| r["message"].as_str()) {
        let message: String = message.chars().take(2000).collect();
        if !lines.contains(&message) {
            lines.push(message);
        }
    }
    if lines.is_empty() {
        let errors = f
            .run_log
            .iter()
            .rev()
            .filter(|l| l.stream == "error")
            .take(5);
        lines = errors
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }
    if f.status == "stopped" {
        lines.retain(|l| !l.contains("failed: refusal"));
    }
    // Zeroshot wraps the provider's own line in generic context, and separately writes an enum
    // summary ("Zeroshot reason ... node outcome ...") that only repeats the title's tooltip;
    // keep the provider's own sentence.
    let mut lines: Vec<String> = lines
        .into_iter()
        .filter_map(|l| clean_error_line(&l))
        .collect();
    // Zeroshot repeats the provider's error inside its own wrapper line; show each text once.
    let all = lines.clone();
    lines.retain(|l| {
        !all.iter()
            .any(|other| other.len() > l.len() && other.contains(l.as_str()))
    });
    // The same paragraph can also be recorded twice verbatim (ledger + terminal context).
    let mut seen = HashSet::new();
    lines.retain(|l| seen.insert(l.clone()));
    lines
}

/// Keeps the provider's own sentence from a Zeroshot-wrapped error line, dropping the generic
/// wrapper ("Node x failed: crash after 12.3s: node execution failed: … provider failure: ",
/// "; provider process exited with status N") and the raw stream-debug dump some crashes repeat
/// ("From chunk: […]"). Drops the enum summary line entirely (its reason/outcome are already in
/// the failure title's tooltip).
fn clean_error_line(text: &str) -> Option<String> {
    if text.starts_with("Zeroshot reason ") && text.contains("node outcome") {
        return None;
    }
    let text = if let Some((_, rest)) = text.rsplit_once("provider failure: ") {
        rest
    } else if let Some((_, rest)) = text.rsplit_once("node execution failed: ") {
        rest
    } else {
        text
    };
    let text = text
        .rsplit_once("; provider process exited with status")
        .map_or(text, |(head, _)| head);
    let text = text
        .find("From chunk:")
        .map_or(text, |i| text[..i].trim_end_matches([' ', ':', '.']));
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// When a run failed.
struct When {
    /// When the deciding node ended (or the controller was last seen), not when Zeroshot finally
    /// closed the run.
    at: Option<u64>,
    /// How long after `at` Zeroshot closed the run, when that took more than five seconds.
    closed_after_ms: Option<u64>,
}

fn when(f: &Facts, culprit: Option<&Execution>) -> When {
    let stalled = f.status == "stalled";
    let culprit_end = culprit.and_then(|e| e.ended_at.or(e.last_activity_at));
    let at = if stalled || f.lost_before_resume {
        culprit
            .and_then(|e| e.last_activity_at.or(e.ended_at))
            .max(f.ended_at)
            .or(culprit_end)
    } else if f.status == "stopped" {
        f.ended_at.or(culprit_end)
    } else {
        culprit_end.or(f.ended_at)
    };
    let closed_after_ms = match (at, f.ended_at) {
        (Some(a), Some(e)) if !stalled && !f.lost_before_resume && e > a + 5_000 => Some(e - a),
        _ => None,
    };
    When {
        at,
        closed_after_ms,
    }
}

/// The culprit as a headline names it: `worker`, `review (reviewer) in round 2`.
fn who(culprit: Option<&Execution>, info: Option<&GraphNode>) -> String {
    let Some(e) = culprit else {
        return "A node".to_owned();
    };
    let role = if info.is_some_and(|n| n.kind == "verifier") {
        " (reviewer)"
    } else {
        ""
    };
    let round = if e.round > 1 {
        format!(" in round {}", e.round)
    } else {
        String::new()
    };
    format!("{}{role}{round}", e.node)
}

/// The company behind a provider (Zeroshot's `provider` id), as a headline names it; `None` for
/// a provider zerochat does not know (headlines then say "the provider"). From the provider,
/// never the harness: Copilot or Claude Code on `anthropic` is still Anthropic.
pub(super) fn company(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "anthropic" => "Anthropic",
        "openai" => "OpenAI",
        "github" => "GitHub",
        "openrouter" => "OpenRouter",
        "bedrock" => "Amazon Bedrock",
        "foundry" => "Microsoft Foundry",
        "opencode" | "opencode-go" => "OpenCode",
        _ => return None,
    })
}

/// The company a headline names: the provider's (`company`), else "the provider".
#[derive(Clone, Copy, Debug)]
pub(super) struct Company(Option<&'static str>);

impl Company {
    pub(super) fn of(provider: Option<&str>) -> Company {
        Company(provider.and_then(company))
    }

    /// Opening a sentence: `Anthropic`, `The provider`.
    pub(super) fn subject(self) -> &'static str {
        self.0.unwrap_or("The provider")
    }

    /// Inside a sentence: `Anthropic`, `the provider`.
    pub(super) fn object(self) -> &'static str {
        self.0.unwrap_or("the provider")
    }

    /// Opening a sentence, possessive: `Anthropic's`, `The provider's`.
    pub(super) fn subject_possessive(self) -> String {
        format!("{}'s", self.subject())
    }

    /// Inside a sentence, before a noun: `the Anthropic usage limit`, `the provider's usage limit`.
    pub(super) fn the(self, noun: &str) -> String {
        match self.0 {
            Some(name) => format!("the {name} {noun}"),
            None => format!("the provider's {noun}"),
        }
    }
}

/// The graph's `fail` node the run ended in: one whose reason matches, the one named after the
/// reason first.
fn fail_node<'a>(f: &Facts<'a>, reason: &str) -> Option<&'a GraphNode> {
    f.nodes
        .values()
        .copied()
        .filter(|n| n.kind == "fail" && n.fail_reason.as_deref() == Some(reason))
        .min_by_key(|n| (n.name != reason, n.name.clone()))
}

/// The other nodes whose work finished before the culprit, in order, each once.
fn kept(executions: &[Execution], culprit: Option<&Execution>) -> Vec<String> {
    let Some(c) = culprit else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    let done = |e: &&Execution| matches!(e.state, ExecState::Succeeded | ExecState::Accepted);
    for e in executions
        .iter()
        .filter(|e| e.id < c.id && e.node != c.node)
        .filter(done)
    {
        if !names.contains(&e.node) {
            names.push(e.node.clone());
        }
    }
    names
}

/// What Zeroshot will accept for this run now: replay, resume, both or neither.
fn actions(f: &Facts, resume_helps: bool) -> Vec<Action> {
    let mut actions = Vec::new();
    // While Zeroshot still closes the run (Active Memory postmortem after the graph failed) it
    // refuses resume and replay ("no recoverable workspace"), so offer them only afterwards.
    let closing = f.phase == "running" && f.status == "failed";
    // Zeroshot replays only finished runs; a stalled run never recorded its end, so resume is the way.
    let stalled = f.status == "stalled";
    if !closing && !stalled && f.local.is_none_or(|l| l.has_checkpoints) {
        actions.push(Action::Replay);
    }
    if !closing && resume_helps && f.local.is_none_or(|l| l.resumed_as.is_none()) {
        actions.push(Action::Resume);
    }
    actions
}

#[cfg(test)]
mod tests;
