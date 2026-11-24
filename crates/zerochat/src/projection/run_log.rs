//! The run log: the run's own timeline. Zeroshot's run-level log lines (most from background
//! agents, streamed as deltas) plus the lines zerochat writes for the run's facts (a node
//! started or finished, a message to the agents, a suspension). The wording of those lines
//! lives here.

use std::collections::HashMap;

use serde_json::Value;

use super::model::{ExecState, Execution, LogLine, PressureLevel};
use super::words::{RESULT_TOOL, error_words};
use crate::engine_lines::{humanize_retry, is_retry_line, parse_rate_limit_line};

/// A streamed delta joins its agent's open line only when it comes within this long of the
/// previous delta ...
const DELTA_JOIN_GAP_MS: u64 = 2_000;
/// ... and while that line is shorter than this (bytes).
const DELTA_JOIN_MAX_LEN: usize = 4_000;

/// The run log as it is built, event by event.
#[derive(Default)]
pub(super) struct RunLog {
    pub(super) lines: Vec<LogLine>,
    /// The line each background agent is still streaming into, per (agent, stream): its index
    /// in `lines` and the time of its last delta.
    open: HashMap<(String, String), (usize, u64)>,
}

impl RunLog {
    /// A line zerochat writes about the run itself.
    pub(super) fn note(&mut self, seq: u64, at: u64, text: String) {
        self.lines.push(LogLine {
            seq,
            at,
            stream: "system".to_owned(),
            agent: None,
            text,
        });
    }

    /// A run-level line Zeroshot logged, readably (some say nothing and are dropped). Background
    /// agents stream text and reasoning as deltas (`irming`, ` to ma`) and interleave with each
    /// other, so a delta joins the line its agent is still writing on the same stream, not just
    /// the previous line.
    pub(super) fn logged(
        &mut self,
        seq: u64,
        at: u64,
        stream: &str,
        kind: Option<&str>,
        agent: Option<&str>,
        text: &str,
    ) {
        let Some(shown) = readable_log_text(kind, text) else {
            return;
        };
        let line = |text: String| LogLine {
            seq,
            at,
            stream: stream.to_owned(),
            agent: agent.map(str::to_owned),
            text,
        };
        let Some(agent) = agent else {
            self.lines.push(line(shown));
            return;
        };
        if !is_delta(kind, stream, text, &shown) {
            self.open.retain(|(a, _), _| a != agent);
            self.lines.push(line(shown));
            return;
        }
        let key = (agent.to_owned(), stream.to_owned());
        if let Some((index, last)) = self.open.get_mut(&key)
            && at.saturating_sub(*last) < DELTA_JOIN_GAP_MS
            && self.lines[*index].text.len() < DELTA_JOIN_MAX_LEN
        {
            self.lines[*index].text.push_str(&shown);
            *last = at;
            return;
        }
        self.open.insert(key, (self.lines.len(), at));
        self.lines.push(line(shown));
    }
}

/// A piece of streamed text or reasoning, shown as written: not an engine line (`Claude …`)
/// and not an error.
fn is_delta(kind: Option<&str>, stream: &str, text: &str, shown: &str) -> bool {
    matches!(kind, None | Some("text") | Some("thinking"))
        && shown == text
        && !text.starts_with("Claude ")
        && stream != "error"
}

/// What a run-level log line says, readably; `None` for lines that say nothing (session ids,
/// rate-limit status lines while the limit is fine, the harness's own "result handed in").
fn readable_log_text(kind: Option<&str>, text: &str) -> Option<String> {
    if kind == Some("session") {
        return None;
    }
    if let Some(parsed) = parse_rate_limit_line(text) {
        // Routine "limit OK" pings, and a warning below the real threshold, say nothing here
        // either: the same rule as the run's pressure strip.
        return (parsed.level != PressureLevel::Info).then_some(parsed.text);
    }
    if kind == Some("retry") || is_retry_line(text) {
        return Some(humanize_retry(text).unwrap_or_else(|| text.to_owned()));
    }
    let text = strip_json_fence(text);
    let json = if text.starts_with('{') {
        serde_json::from_str::<Value>(text).ok()
    } else {
        None
    };
    if let Some(v) = &json {
        if v.get("sessionId").is_some() && v.get("harness").is_some() {
            return None;
        }
        if kind == Some("tool_use") || (v.get("name").is_some() && v.get("input").is_some()) {
            let name = v["name"].as_str().unwrap_or("?");
            // The harness's own result-reporting call, not a meaningful step of work: the fact
            // worth a line is that it answered, not the envelope it answered in.
            if name == RESULT_TOOL {
                return Some("reported its result".to_owned());
            }
            let input = v["input"].to_string();
            let input: String = input.chars().take(160).collect();
            return Some(format!("tool {name}: {input}"));
        }
        if kind == Some("tool_result") || v.get("toolUseId").is_some() {
            let content = match &v["content"] {
                Value::String(s) => s.clone(),
                Value::Array(items) => items
                    .iter()
                    .filter_map(|i| i["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                other => other.to_string(),
            };
            if content.starts_with("Structured output provided") {
                return None;
            }
            let content: String = content.chars().take(300).collect();
            return Some(if v["isError"].as_bool() == Some(true) {
                format!("tool error: {content}")
            } else {
                format!("tool result: {content}")
            });
        }
    }
    Some(text.to_owned())
}

/// A fenced block (` ```json\n{…}\n``` `) is the same JSON underneath with markdown wrapped
/// around it for nothing: the run log is not a chat transcript, so the fence itself is noise.
fn strip_json_fence(text: &str) -> &str {
    let Some(rest) = text.trim().strip_prefix("```") else {
        return text;
    };
    let rest = rest
        .strip_prefix("json")
        .unwrap_or(rest)
        .trim_start_matches(['\n', '\r']);
    rest.strip_suffix("```").map_or(text, str::trim_end)
}

// The lines zerochat writes about the run's own facts.

/// `worker started (claude-haiku-4-5)`.
pub(super) fn node_started(e: &Execution) -> String {
    let model = e
        .model
        .as_ref()
        .map(|m| format!(" ({m})"))
        .unwrap_or_default();
    format!("{} started{model}", e.node)
}

/// `worker failed: timed out`, `review rejected`, `worker finished`.
pub(super) fn node_finished(e: &Execution) -> String {
    match e.state {
        ExecState::Failed => {
            let reason = e
                .error
                .as_ref()
                .and_then(|v| v["code"].as_str())
                .map(error_words);
            format!(
                "{} failed{}",
                e.node,
                reason.map(|r| format!(": {r}")).unwrap_or_default()
            )
        }
        ExecState::Rejected => format!("{} rejected", e.node),
        _ => format!("{} finished", e.node),
    }
}

/// A person's steer message: `Message to worker: <its first line>` (`all agents` when it names
/// no node).
pub(super) fn steer_sent(vector: &Value) -> String {
    let to = vector["node"]
        .as_str()
        .map_or_else(|| "all agents".to_owned(), str::to_owned);
    format!(
        "Message to {to}: {}",
        first_line_of(vector["text"].as_str().unwrap_or(""))
    )
}

/// An agent took a steer message in (`None` when the receipt matches no known execution).
pub(super) fn steer_received(node: Option<&str>) -> String {
    match node {
        Some(node) => format!("{node} received the message"),
        None => "An agent received the message".to_owned(),
    }
}

pub(super) const SUSPEND_REQUESTED: &str =
    "Suspend requested: the run pauses at its next checkpoint";

/// `Suspended (request)`, from Zeroshot's suspension record.
pub(super) fn suspended(suspension: &Value) -> String {
    let reason = suspension["reason"]
        .as_str()
        .or_else(|| suspension["kind"].as_str());
    format!(
        "Suspended{}",
        reason
            .map(|r| format!(" ({})", r.replace('_', " ")))
            .unwrap_or_default()
    )
}

/// A node waiting: for an answer to its request, or for its timed wait.
pub(super) fn deferred(node: &str, for_answer: bool) -> String {
    let what = if for_answer {
        "waits for an answer"
    } else {
        "waits until its time comes"
    };
    format!("{node} {what}")
}

/// The first non-empty line, at most 120 characters.
fn first_line_of(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.chars().count() > 120 {
        format!("{}…", line.chars().take(119).collect::<String>())
    } else {
        line.to_owned()
    }
}
