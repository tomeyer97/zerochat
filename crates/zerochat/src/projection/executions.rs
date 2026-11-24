//! Executions: how one starts, what its log lines and outcome say about it, and how it is
//! settled once all events are in.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::model::{CostSource, ExecState, Execution, UsageSource};
use crate::engine_lines::{RATE_LIMIT_PREFIX, is_closing_line, is_retry_line, reported_duration};
use crate::pricing::{Usage, estimate_usd};
use crate::transcript::ToolLine;

/// How many error lines an execution keeps (the latest ones) ...
const MAX_ERROR_LINES: usize = 8;
/// ... and how much of each (characters).
const MAX_ERROR_LINE_CHARS: usize = 2000;

/// Where an execution starts, read from either event that starts one: `node_started` (it runs
/// here) or `prior_execution` (it ran in the run this one replays or resumes, and is reused).
pub(super) struct StartedRef {
    pub(super) id: u64,
    pub(super) node: String,
    pub(super) node_instance: u64,
    pub(super) attempt: u64,
    pub(super) map_indices: Value,
    pub(super) input: Value,
    pub(super) reused: bool,
}

impl StartedRef {
    /// `None` when the event names no execution.
    pub(super) fn from_event(body: &Value) -> Option<Self> {
        let reused = body["kind"] == "prior_execution";
        let (reference, occurrence, attempt, input) = if reused {
            let e = &body["execution"];
            let reference = json!({"node": e["occurrence"]["node"], "nodeInstance": e["node_instance"], "execution": e["execution"]});
            (reference, &e["occurrence"], &e["attempt"], &e["input"])
        } else {
            (
                body["reference"].clone(),
                &body["occurrence"],
                &body["attempt"],
                &body["input"],
            )
        };
        Some(StartedRef {
            id: reference["execution"].as_u64()?,
            node: reference["node"].as_str().unwrap_or("").to_owned(),
            node_instance: reference["nodeInstance"].as_u64().unwrap_or(0),
            attempt: attempt.as_u64().unwrap_or(1),
            map_indices: occurrence.get("mapIndices").cloned().unwrap_or(json!([])),
            input: input.clone(),
            reused,
        })
    }
}

impl Execution {
    /// A new execution, running: everything it has not done yet is empty.
    pub(super) fn started(start: StartedRef, round: u64, start_seq: u64) -> Self {
        Execution {
            id: start.id,
            node: start.node,
            node_instance: start.node_instance,
            attempt: start.attempt,
            map_indices: start.map_indices,
            round,
            start_seq,
            input: start.input,
            reused: start.reused,
            ..Default::default()
        }
    }

    /// Settles a reused execution: it ran in the original run, and the ledger here has no
    /// times for it.
    pub(super) fn reuse(&mut self, seq: u64, outcome: &Value) {
        self.started_at = None;
        self.times_approximate = false;
        self.state = ExecState::Succeeded;
        self.end_seq = Some(seq);
        self.apply_outcome(outcome);
    }

    /// Takes in Zeroshot's outcome: the state, the output, a verifier's verdict, an error.
    pub(super) fn apply_outcome(&mut self, outcome: &Value) {
        self.outcome = Some(outcome.clone());
        self.output = outcome.get("output").filter(|o| !o.is_null()).cloned();
        match outcome["status"].as_str().unwrap_or("") {
            "verified" => self.state = ExecState::Succeeded,
            "verifier" => {
                let signals = outcome["signals"].clone();
                let accepted = verdict_accepted(&signals);
                self.signals = Some(signals);
                self.diagnostic = outcome.get("diagnostic").filter(|d| !d.is_null()).cloned();
                self.accepted = accepted;
                self.state = if accepted == Some(false) {
                    ExecState::Rejected
                } else {
                    ExecState::Accepted
                };
            }
            "error" => {
                self.state = ExecState::Failed;
                self.error = Some(json!({"code": outcome["code"], "reason": outcome["reason"]}));
            }
            _ => self.state = ExecState::Succeeded,
        }
    }

    /// Counts one of its log lines: activity, tool calls, retries, its session id and error
    /// lines (with Zeroshot's own duration, when a failure line reports one).
    pub(super) fn note_log_line(&mut self, line: &LoggedLine) {
        self.log_lines += 1;
        // Zeroshot closing the run is not the agent being active.
        if !is_closing_line(line.text) {
            self.last_activity_at = Some(line.at);
        }
        if line.is_tool_call() {
            self.tool_calls += 1;
        }
        // A tool call runs until its result; after a result (or the agent's own text) the model
        // owes the next answer. Retries, limit notices and errors say nothing about whose turn it is.
        if line.is_tool_call() {
            self.model_turn_since = None;
        } else if line.at > 0
            && !line.is_error()
            && !line.is_retry()
            && !line.is_notice()
            && !is_closing_line(line.text)
        {
            self.model_turn_since = Some(line.at);
        }
        // Only genuine retry attempts count; routine "status=allowed" rate-limit pings are not
        // something this execution had to survive.
        if line.is_retry() {
            self.retries += 1;
        }
        if line.kind == Some("session")
            && let Ok(v) = serde_json::from_str::<Value>(line.text)
        {
            self.session_id = v["sessionId"].as_str().map(str::to_owned);
        }
        if line.is_error() {
            if !self.error_lines.iter().any(|l| l == line.text) {
                if self.error_lines.len() == MAX_ERROR_LINES {
                    self.error_lines.remove(0);
                }
                self.error_lines
                    .push(line.text.chars().take(MAX_ERROR_LINE_CHARS).collect());
            }
            if let Some(ms) = reported_duration(line.text) {
                self.reported_ms = Some(ms);
            }
        }
    }
}

/// One `safe_log` line, as an execution counts it.
pub(super) struct LoggedLine<'a> {
    pub(super) at: u64,
    pub(super) text: &'a str,
    pub(super) stream: &'a str,
    /// The older patched Zeroshot's line kind (`tool_use`, `text`, `retry`, `session`, …).
    pub(super) kind: Option<&'a str>,
    /// A Zeroshot 16.1 tool line (`Claude tool call: …`), which carries no kind.
    pub(super) tool: Option<ToolLine<'a>>,
    /// The line carries a diagnostic.
    pub(super) diagnostic: bool,
}

impl LoggedLine<'_> {
    fn is_tool_call(&self) -> bool {
        self.kind == Some("tool_use") || matches!(self.tool, Some(ToolLine::Call { .. }))
    }

    fn is_retry(&self) -> bool {
        self.kind == Some("retry") || is_retry_line(self.text)
    }

    fn is_error(&self) -> bool {
        self.stream == "error" || self.diagnostic
    }

    /// A rate-limit notice (`Claude rate limit: status=allowed, …`) or the agent's session id.
    fn is_notice(&self) -> bool {
        matches!(self.kind, Some("rate_limit") | Some("session"))
            || self.text.starts_with(RATE_LIMIT_PREFIX)
    }

    /// The agent is visibly working: a tool call or result, streamed text or reasoning.
    pub(super) fn shows_activity(&self) -> bool {
        matches!(
            self.kind,
            Some("tool_use") | Some("tool_result") | Some("text") | Some("thinking")
        ) || self.tool.is_some()
    }
}

/// A signal that reads like a rejection (lower-case).
const REJECT_WORDS: [&str; 12] = [
    "reject",
    "rejected",
    "fail",
    "failed",
    "changes_requested",
    "needs_changes",
    "blocked",
    "deny",
    "denied",
    "repair_required",
    "conflict",
    "ci_failed",
];

/// A signal that reads like a pass (lower-case).
const ACCEPT_WORDS: [&str; 12] = [
    "accept",
    "accepted",
    "approve",
    "approved",
    "pass",
    "passed",
    "ok",
    "success",
    "succeeded",
    "merged",
    "opened",
    "converged",
];

/// A verifier is accepted when every signal reads like a pass; a signal that reads like a
/// rejection rejects; anything else is undecided (`None`).
pub fn verdict_accepted(signals: &Value) -> Option<bool> {
    let map = signals.as_object()?;
    let mut any_pass = false;
    for value in map.values() {
        let label = value.as_str().unwrap_or("").to_ascii_lowercase();
        if REJECT_WORDS.contains(&label.as_str()) {
            return Some(false);
        }
        if ACCEPT_WORDS.contains(&label.as_str()) {
            any_pass = true;
        }
    }
    any_pass.then_some(true)
}

/// What settling an execution needs to know about the run.
pub(super) struct RunEnd<'a> {
    /// Zeroshot finished (or suspended) the run.
    pub(super) finished: bool,
    pub(super) stalled: bool,
    pub(super) stopped: bool,
    /// Zeroshot closed the run as lost (`runtime_lost`): its controller died.
    pub(super) lost: bool,
    /// The ledger position of the force stop, if one was requested.
    pub(super) force_stop_seq: Option<u64>,
    /// Usage read live from the providers' session logs, by execution id.
    pub(super) live_usage: &'a HashMap<u64, Usage>,
    /// The run reused executions come from.
    pub(super) reused_from: Option<String>,
    pub(super) now_ms: u64,
}

/// Settles an execution once all events are in. The order matters: the state first (an
/// interrupted execution ends at its last activity), then usage (live usage counts only for
/// executions that were not voided), then cost (from the usage), then the times.
pub(super) fn settle(e: &mut Execution, run: &RunEnd) {
    settle_state(e, run);
    settle_usage(e, run);
    settle_cost(e);
    settle_times(e, run);
}

/// An execution still running when the run ended was interrupted; so was one that failed
/// because of a force stop, and one Zeroshot failed only to close a lost run
/// (`Node worker failed: crash: run runtime_lost`, written when the run is resumed, minutes
/// after the agent was last active). An interrupted execution ends at its last activity.
fn settle_state(e: &mut Execution, run: &RunEnd) {
    let closed_as_lost = run.lost
        && e.state == ExecState::Failed
        && e.error_lines.iter().any(|l| is_closing_line(l));
    if (e.state == ExecState::Running && (run.finished || run.stalled)) || closed_as_lost {
        e.state = ExecState::Interrupted;
        e.ended_at = e.last_activity_at.or(e.started_at);
    }
    // Zeroshot records a node it force-stopped as a `refusal`; it did not refuse anything.
    // A sibling that Zeroshot cancelled for the stop may be recorded before the stop event
    // itself; its outcome is the same `refusal`, so it was interrupted too.
    let refusal = e.error.as_ref().is_some_and(|v| v["code"] == "refusal");
    let ended_after_stop = run
        .force_stop_seq
        .is_some_and(|f| e.end_seq.is_some_and(|end| end > f));
    if e.state == ExecState::Failed && run.stopped && (refusal || ended_after_stop) {
        e.state = ExecState::Interrupted;
    }
}

/// Until Zeroshot counts an execution's tokens, its usage comes live from the provider's
/// session log, or is pending while it runs.
fn settle_usage(e: &mut Execution, run: &RunEnd) {
    let ledger_usage = e.usage.total() > 0;
    if ledger_usage || e.reused || e.state == ExecState::Voided {
        return;
    }
    if let Some(live) = run.live_usage.get(&e.id).filter(|u| u.total() > 0) {
        e.usage = *live;
        e.usage_source = UsageSource::SessionLog;
    } else if e.state == ExecState::Running {
        e.usage_source = UsageSource::Pending;
    }
}

/// Without a provider cost, the cost is estimated from the tokens and the model.
fn settle_cost(e: &mut Execution) {
    if e.cost_usd.is_some() || e.usage.total() == 0 {
        return;
    }
    if let Some(model) = &e.model {
        e.cost_usd = estimate_usd(model, &e.usage, e.harness.as_deref());
        e.cost_source = e.cost_usd.map(|_| CostSource::Estimate);
    }
}

/// One duration per execution: Zeroshot's own figure when it reported one, else its times (up
/// to now while it runs). A reused execution has no times here, only the run it came from.
fn settle_times(e: &mut Execution, run: &RunEnd) {
    if let (Some(reported), true) = (e.reported_ms, e.times_approximate)
        && let Some(end) = e.ended_at
    {
        e.started_at = Some(end.saturating_sub(reported));
    }
    let end = match e.state {
        ExecState::Running => Some(run.now_ms),
        _ => e.ended_at,
    };
    if let (Some(start), Some(end)) = (e.started_at, end) {
        e.duration_ms = Some(end.saturating_sub(start));
    }
    if e.reused {
        e.duration_ms = None;
        e.started_at = None;
        e.ended_at = None;
        e.reused_from = run.reused_from.clone();
    }
}
