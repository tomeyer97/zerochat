//! Limits close to running out on a live run: a node's time, attempts and loop rounds, and the
//! provider's rate limits and retries. Healthy limits are not shown, and a finished run has
//! none.

use std::collections::{BTreeMap, HashMap};

use super::graph::GraphNode;
use super::model::{ExecState, Execution, Pressure, PressureKind, PressureLevel};
use super::words::human_ms;
use crate::engine_lines::{
    RATE_LIMIT_TITLE, RETRY_TITLE, humanize_retry, is_retry_line, parse_rate_limit_line,
};

impl Pressure {
    /// A warning about one execution of `node`.
    fn warning(kind: PressureKind, execution: &Execution, text: String) -> Self {
        Pressure {
            kind,
            level: PressureLevel::Warning,
            text,
            execution: Some(execution.id),
            node: Some(execution.node.clone()),
            at: None,
            resets_at: None,
        }
    }
}

/// One log line, as far as limits go.
pub(super) struct ApiLine<'a> {
    pub(super) text: &'a str,
    /// `output` (the agent's text), `system`, `error`.
    pub(super) stream: &'a str,
    /// Zeroshot's structured line kind (`retry`, `rate_limit`, `tool_result`, …), when it
    /// records one.
    pub(super) kind: Option<&'a str>,
    pub(super) at: u64,
}

/// The pressures seen in the run's log lines, as the events are folded.
#[derive(Default)]
pub(super) struct Pressures {
    /// Retries and limit errors, one per kind and execution.
    seen: Vec<Pressure>,
    /// The latest rate-limit notice per window (`5-hour`, `7-day`, …).
    rate_limits: BTreeMap<String, Pressure>,
}

impl Pressures {
    /// Notes what one log line says about limits. Only the agent's own API events count:
    /// Zeroshot's `Claude rate limit: …` and `Claude API retry n/max: …` notices, and its
    /// structured `rate_limit`/`retry` lines. Tool output and the agent's text are content,
    /// never pressure, whatever words they contain (an agent that greps logs or tests an HTTP
    /// client prints "429 rate_limit_error" without being limited). A pressure's text is
    /// always a title: a line this parser cannot read shows as `rate limit hit` or
    /// `retrying the provider's API`, never as itself.
    pub(super) fn observe(&mut self, line: &ApiLine, execution: Option<u64>, node: Option<String>) {
        // The agent's own text (stream `output`) can say anything; Zeroshot's notices are
        // `system` lines.
        let notice = line.stream != "output";
        if notice && let Some(parsed) = parse_rate_limit_line(line.text) {
            let pressure = Pressure {
                kind: PressureKind::RateLimit,
                level: parsed.level,
                text: parsed.text,
                execution,
                node,
                at: Some(line.at),
                resets_at: parsed.resets_at,
            };
            self.rate_limits.insert(parsed.window, pressure);
            return;
        }
        let retry = line.kind == Some("retry") || (notice && is_retry_line(line.text));
        let limit = line.kind == Some("rate_limit");
        if !(retry || limit) {
            return;
        }
        let (kind, level, text) = if limit {
            (
                PressureKind::RateLimit,
                PressureLevel::Critical,
                RATE_LIMIT_TITLE.to_owned(),
            )
        } else {
            (
                PressureKind::Retry,
                PressureLevel::Warning,
                humanize_retry(line.text).unwrap_or_else(|| RETRY_TITLE.to_owned()),
            )
        };
        self.seen
            .retain(|p| !(p.kind == kind && p.execution == execution));
        self.seen.push(Pressure {
            kind,
            level,
            text,
            execution,
            node,
            at: Some(line.at),
            resets_at: None,
        });
    }

    /// A retry is pressing only while the agent waits on it: once the execution works again
    /// (a model call answered, a tool call, streamed text) its retry pressure clears.
    pub(super) fn clear_retry(&mut self, execution: u64) {
        self.seen
            .retain(|p| !(p.kind == PressureKind::Retry && p.execution == Some(execution)));
    }

    /// The pressures to show. A run that is not live has none; on a live one: what the log
    /// showed, each running execution's own limits, and rate limits past their warning
    /// threshold. A retry shows only while its execution still runs.
    pub(super) fn finish(
        self,
        live: bool,
        executions: &[Execution],
        nodes: &HashMap<&str, &GraphNode>,
        now_ms: u64,
    ) -> Vec<Pressure> {
        if !live {
            return Vec::new();
        }
        let mut pressures = self.seen;
        let mut loops_seen: Vec<String> = Vec::new();
        for e in executions.iter().filter(|e| e.state == ExecState::Running) {
            let Some(info) = nodes.get(e.node.as_str()) else {
                continue;
            };
            pressures.extend(node_pressures(e, info, nodes, now_ms, &mut loops_seen));
        }
        pressures.extend(
            self.rate_limits
                .into_values()
                .filter(|p| p.level != PressureLevel::Info),
        );
        let running = |id: Option<u64>| {
            executions
                .iter()
                .any(|e| Some(e.id) == id && e.state == ExecState::Running)
        };
        pressures.retain(|p| p.kind != PressureKind::Retry || running(p.execution));
        pressures
    }
}

/// A running execution's own limits:
/// - timeout: less than a quarter of its node's time is left;
/// - attempts: it is the node's last attempt (of more than one);
/// - rounds: its loop is in its last round (once per loop, `loops_seen`).
fn node_pressures(
    e: &Execution,
    info: &GraphNode,
    nodes: &HashMap<&str, &GraphNode>,
    now_ms: u64,
    loops_seen: &mut Vec<String>,
) -> Vec<Pressure> {
    let mut out = Vec::new();
    if let (Some(timeout), Some(start)) = (info.timeout_ms, e.started_at) {
        let left = timeout.saturating_sub(now_ms.saturating_sub(start));
        if left < timeout / 4 {
            out.push(Pressure::warning(
                PressureKind::Timeout,
                e,
                format!("timeout in {} (of {})", human_ms(left), human_ms(timeout)),
            ));
        }
    }
    if let Some(max) = info.attempts.filter(|m| *m > 1 && e.attempt >= *m) {
        out.push(Pressure::warning(
            PressureKind::Attempts,
            e,
            format!("last attempt ({} of {max})", e.attempt),
        ));
    }
    let last_round_of = info
        .loops
        .last()
        .and_then(|name| Some((name, nodes.get(name.as_str())?.max_iterations?)))
        .filter(|(name, max)| e.round >= *max && !loops_seen.contains(name));
    if let Some((loop_name, max)) = last_round_of {
        loops_seen.push(loop_name.clone());
        out.push(Pressure::warning(
            PressureKind::Rounds,
            e,
            format!("last round of {loop_name} ({} of {max})", e.round),
        ));
    }
    out
}
