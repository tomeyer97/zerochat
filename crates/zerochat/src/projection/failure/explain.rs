//! The reason Zeroshot closed a run, explained: what kind of failure it is, its cause and
//! headline, what to fix first and which action fits best. A failed agent's own cause is worded
//! in agent_copy.rs.

use serde_json::Value;

use super::super::graph::GraphNode;
use super::super::model::{Action, ExecState, Execution, Feedback, LogLine};
use super::agent_copy::explain_agent_failure;
use super::{Company, Facts};
use crate::source::Liveness;

/// What `explain` reads: the run's facts and what `classify` found out about the culprit.
pub(super) struct Case<'a> {
    pub(super) f: &'a Facts<'a>,
    /// Zeroshot's reason for closing the run (`controller_not_running` for a stalled run).
    pub(super) reason: &'a str,
    /// The execution that decided the outcome.
    pub(super) culprit: Option<&'a Execution>,
    /// The culprit's graph node.
    pub(super) info: Option<&'a GraphNode>,
    /// The culprit as a headline names it (`A node` without one).
    pub(super) who: String,
    /// The culprit's harness, as its lane names it (`None`: the ledger does not say).
    pub(super) harness: Option<&'a str>,
    /// The company behind the culprit's provider.
    pub(super) company: Company,
    pub(super) last_errors: &'a [String],
    /// When the run failed.
    pub(super) at: Option<u64>,
}

/// The kind of failure, as the failure card's `class` names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Class {
    /// The work was not accepted.
    Work,
    /// An agent failed.
    Agent,
    /// The provider's side: not answering, a broken or malformed stream, overloaded or down, a
    /// usage or rate limit. Usually temporary: resume (at the reset time, for a limit).
    Provider,
    /// Credentials, model access, disk or a missing CLI: fix the setup first.
    Setup,
    /// Zeroshot itself (controller, runtime, checkout).
    Orchestration,
    /// Pushing the change or opening its pull request.
    Delivery,
    /// Someone stopped the run.
    Stopped,
}

impl Class {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Class::Work => "work",
            Class::Agent => "agent",
            Class::Provider => "provider",
            Class::Setup => "setup",
            Class::Orchestration => "orchestration",
            Class::Delivery => "delivery",
            Class::Stopped => "stopped",
        }
    }
}

/// A failure explained: the copy of the failure card and the action it leads with.
pub(super) struct Explanation {
    pub(super) class: Class,
    /// The cause in 2-4 words, for run lists.
    pub(super) cause: String,
    /// One short headline.
    pub(super) title: String,
    /// At most one sentence the headline does not already say.
    pub(super) detail: String,
    /// What to fix before retrying, when retrying unchanged would fail the same way.
    pub(super) fix: Option<String>,
    /// The command that makes the fix.
    pub(super) fix_command: Option<String>,
    /// The action that fits the cause best.
    pub(super) primary: Option<Action>,
    /// Resuming reuses the graph, prompts and models unchanged; when the cause is one of those
    /// (model, timeout), it can only repeat the failure.
    pub(super) resume_helps: bool,
    /// When a usage limit resets (Unix ms).
    pub(super) resets_at: Option<u64>,
    /// Reviewers' rejections, for work that was not accepted.
    pub(super) feedback: Vec<Feedback>,
}

impl Explanation {
    pub(super) fn new(
        class: Class,
        cause: impl Into<String>,
        title: impl Into<String>,
    ) -> Explanation {
        Explanation {
            class,
            cause: cause.into(),
            title: title.into(),
            detail: String::new(),
            fix: None,
            fix_command: None,
            primary: None,
            resume_helps: true,
            resets_at: None,
            feedback: Vec::new(),
        }
    }

    pub(super) fn detail(self, detail: impl Into<String>) -> Explanation {
        Explanation {
            detail: detail.into(),
            ..self
        }
    }

    pub(super) fn fix(self, fix: impl Into<String>) -> Explanation {
        Explanation {
            fix: Some(fix.into()),
            ..self
        }
    }

    pub(super) fn fix_command(self, command: impl Into<String>) -> Explanation {
        Explanation {
            fix_command: Some(command.into()),
            ..self
        }
    }

    pub(super) fn primary(self, action: Action) -> Explanation {
        Explanation {
            primary: Some(action),
            ..self
        }
    }

    /// Resuming unchanged would fail the same way.
    pub(super) fn resume_does_not_help(self) -> Explanation {
        Explanation {
            resume_helps: false,
            ..self
        }
    }

    pub(super) fn resets_at(self, resets_at: Option<u64>) -> Explanation {
        Explanation { resets_at, ..self }
    }
}

/// Explains the reason Zeroshot closed the run.
pub(super) fn explain(c: &Case) -> Explanation {
    let suspended =
        c.f.local
            .is_some_and(|l| l.controller_liveness == Liveness::Suspended);
    match c.reason {
        "force_stopped" => Explanation::new(Class::Stopped, "stopped", "Stopped by a force stop"),
        "controller_not_running" if suspended => controller_suspended(c),
        "controller_not_running" => controller_lost(c),
        "runtime_lost" if c.f.lost_before_resume => {
            Explanation::new(Class::Orchestration, "controller lost", "Controller lost")
        }
        "runtime_lost" | "runtime_failed" | "runtime_unavailable" => runtime_failed(c),
        "source_checkout_unavailable" => Explanation::new(
            Class::Orchestration,
            "checkout unavailable",
            "Zeroshot could not prepare the repository",
        ),
        "delivery_unconfirmed" | "delivery_failed" => {
            Explanation::new(Class::Delivery, "delivery failed", "The change could not be delivered").detail(
                "Pushing or opening the pull request failed or could not be confirmed. The work is in the workspace.",
            )
        }
        "attempts_exhausted" => {
            let attempts = c.info.and_then(|n| n.attempts).unwrap_or(1);
            let title = format!("{} failed on every attempt ({attempts})", c.who);
            Explanation::new(Class::Agent, "failed every attempt", title).primary(Action::Replay)
        }
        _ => graph_ended(c),
    }
}

/// The controller was stopped with SIGSTOP: nothing progresses until it continues.
fn controller_suspended(c: &Case) -> Explanation {
    let pid = c.f.local.and_then(|l| l.controller_pid).unwrap_or(0);
    Explanation::new(
        Class::Orchestration,
        "controller suspended",
        "The run's controller is suspended",
    )
    .detail(format!(
        "Controller pid {pid} is stopped (SIGSTOP), so nothing can progress."
    ))
    .fix(format!(
        "Continue it with `kill -CONT {pid}`, or force stop the run."
    ))
}

/// The controller is gone while the run was still going (a stalled run).
fn controller_lost(c: &Case) -> Explanation {
    let recorded = runtime_failure(c.f.run_log, c.f.terminal);
    let agents =
        c.f.local
            .map(|l| l.agent_processes.as_slice())
            .unwrap_or_default();
    let detail = match agents.first() {
        Some(p) => format!(
            "The controller is gone, but an agent process ({} pid {}) is still running and changing the \
             workspace. Stop it (kill {}) before you resume, or two agents will edit the same files.",
            p.name, p.pid, p.pid
        ),
        None => recorded
            .as_ref()
            .map(|e| format!("Zeroshot recorded: {e}"))
            .unwrap_or_default(),
    };
    match recorded.as_deref().and_then(runtime_cause) {
        Some(known) => known.explain(detail),
        None => Explanation::new(Class::Orchestration, "controller lost", "Controller lost")
            .detail(detail)
            .primary(Action::Resume),
    }
}

/// Zeroshot's runtime failed or went away. Zeroshot often knows the precise cause ("Runtime
/// failed: …") even when it closes the run as `runtime_lost`; that beats a generic crash story.
fn runtime_failed(c: &Case) -> Explanation {
    let recorded = runtime_failure(c.f.run_log, c.f.terminal);
    if let Some(r) = &recorded
        && let Some(known) = runtime_cause(r)
    {
        return known.explain(format!("Zeroshot recorded: {r}"));
    }
    let explanation = match recorded {
        None if c.reason == "runtime_lost" => Explanation::new(
            Class::Orchestration,
            "runtime lost",
            "Zeroshot lost the run's runtime",
        )
        .detail("The process or machine running the agents went away before the run finished."),
        recorded => Explanation::new(
            Class::Orchestration,
            "runtime failed",
            "Zeroshot's runtime failed",
        )
        .detail(
            recorded
                .map(|r| format!("Zeroshot recorded: {r}"))
                .unwrap_or_else(|| "Zeroshot could not keep running the graph.".into()),
        ),
    };
    explanation.primary(Action::Resume)
}

/// Any other reason: the work was not accepted, an agent failed, or the graph ended in a reason
/// of its own (a `fail` node).
fn graph_ended(c: &Case) -> Explanation {
    let culprit_failed = c
        .culprit
        .is_some_and(|e| matches!(e.state, ExecState::Failed | ExecState::Interrupted));
    let culprit_rejected = c.culprit.is_some_and(|e| e.state == ExecState::Rejected);
    if c.reason.ends_with("_exhausted") || (culprit_rejected && !culprit_failed) {
        return not_accepted(c);
    }
    match c.culprit {
        Some(e) if culprit_failed => explain_agent_failure(c, e),
        _ => Explanation::new(
            Class::Work,
            c.reason.replace('_', " "),
            format!("The graph ended in {}", c.reason),
        ),
    }
}

/// The reviewers did not accept the work: how many rounds it took, and their last rejections.
fn not_accepted(c: &Case) -> Explanation {
    let loop_info = c
        .info
        .and_then(|n| n.loops.last())
        .and_then(|l| c.f.nodes.get(l.as_str()));
    let (round, max) = match (c.culprit, loop_info) {
        (Some(e), Some(l)) => (e.round, l.max_iterations.unwrap_or(e.round)),
        (Some(e), None) => (e.round, e.round),
        _ => (0, 0),
    };
    let rejections =
        c.f.executions
            .iter()
            .rev()
            .filter(|e| e.state == ExecState::Rejected)
            .take(6);
    let feedback = rejections.map(rejection_feedback).collect();
    let (cause, title) = match round {
        0 => (
            "not accepted".to_owned(),
            "The work was not accepted".to_owned(),
        ),
        1 => (
            "not accepted after 1 round".to_owned(),
            format!("Not accepted after 1 of {max} rounds"),
        ),
        _ => (
            format!("not accepted after {round} rounds"),
            format!("Not accepted after {round} of {max} rounds"),
        ),
    };
    // A replay reruns the same reviewer on the same rounds: it rejects again. The fix is in the
    // graph (more rounds, or what the reviewer asks for), so it leads instead of a replay.
    let reviewer = c
        .culprit
        .map(|e| format!("`{}`", e.node))
        .unwrap_or_else(|| "the reviewer".to_owned());
    let fix = match (loop_info, c.culprit) {
        (Some(l), Some(_)) if round > 0 => format!(
            "Raise `maxIterations` on `{}` in your graph (now {max}), or change what {reviewer} asks for; then start a new run.",
            l.name
        ),
        _ => format!("Change what {reviewer} asks for in your graph, then start a new run."),
    };
    Explanation {
        feedback,
        ..Explanation::new(Class::Work, cause, title).fix(fix)
    }
}

/// A rejection as the failure card shows it: the verdict signals and the reviewer's message.
fn rejection_feedback(e: &Execution) -> Feedback {
    let text = match &e.diagnostic {
        Some(Value::String(s)) => s.clone(),
        Some(d) => d
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| d.to_string()),
        None => String::new(),
    };
    let signals = e.signals.as_ref().and_then(Value::as_object);
    let verdict = signals
        .map(|m| {
            m.values()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Feedback {
        node: e.node.clone(),
        round: e.round,
        execution: e.id,
        verdict,
        text,
    }
}

/// The runtime error Zeroshot recorded for the run: the terminal's primary message, else the last
/// `Runtime failed: …` line of the run log.
fn runtime_failure(run_log: &[LogLine], terminal: &Value) -> Option<String> {
    let primary = terminal
        .pointer("/context/primary/message")
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty() && !m.starts_with("Run failed"));
    primary
        .map(str::to_owned)
        .or_else(|| {
            run_log
                .iter()
                .rev()
                .find(|l| l.text.starts_with("Runtime failed:"))
                .map(|l| l.text.clone())
        })
        .map(|m| m.chars().take(300).collect())
}

/// A runtime error Zeroshot's own message names.
struct RuntimeCause {
    cause: &'static str,
    title: &'static str,
    /// What to fix first: resuming unchanged would fail the same way.
    fix: Option<&'static str>,
}

impl RuntimeCause {
    /// Resume is the way unless something must be fixed first.
    fn explain(self, detail: String) -> Explanation {
        let explanation =
            Explanation::new(Class::Orchestration, self.cause, self.title).detail(detail);
        match self.fix {
            Some(fix) => explanation.fix(fix),
            None => explanation.primary(Action::Resume),
        }
    }
}

/// Known runtime errors, matched on Zeroshot's message (a provider's error text is matched in
/// diagnose.rs).
fn runtime_cause(message: &str) -> Option<RuntimeCause> {
    let lower = message.to_ascii_lowercase();
    let known = |cause, title, fix| Some(RuntimeCause { cause, title, fix });
    if lower.contains("exceeds the storage bound") {
        return known(
            "input too large",
            "The run's input or a node's output is too large for Zeroshot",
            Some(
                "Shorten the task input (Zeroshot stores at most 1 MiB per event), then replay or start a new run.",
            ),
        );
    }
    if lower.contains("permission denied") {
        return known(
            "workspace not writable",
            "Zeroshot could not write the workspace checkpoint",
            Some("Make the workspace (and Zeroshot's state directory) writable, then resume."),
        );
    }
    // Broader than diagnose's `Disk` needles (no "on device"); kept apart so neither changes.
    if lower.contains("no space left") || lower.contains("enospc") {
        return known(
            "disk full",
            "The disk is full",
            Some("Free disk space, then resume."),
        );
    }
    if lower.contains("checkpoint") || lower.contains("restic") {
        return known(
            "checkpoint failed",
            "Zeroshot could not save a workspace checkpoint",
            None,
        );
    }
    None
}
