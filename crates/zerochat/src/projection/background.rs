//! Background agents: the agents Zeroshot runs outside the graph (Active Memory, Zeroshot's
//! own memory maintenance, is one), seen only through their `[name] …` log lines.

use std::collections::BTreeMap;

use super::model::{BackgroundAgent, LogLine};
use crate::engine_lines::is_retry_line;

/// Zeroshot's run-level lines about its Active Memory agents: `Active Memory <name> <what>`.
pub(super) const ACTIVE_MEMORY_PREFIX: &str = "Active Memory ";

/// Starts the run-log line of Active Memory's postmortem: after a graph fails, Zeroshot has it
/// study the failure before it closes the run, which can take a minute or more.
pub(super) const POSTMORTEM_PREFIX: &str = "Active Memory postmortem";

/// A background agent's line names it in brackets: `[a_graph] text` → (`a_graph`, `text`).
pub(super) fn split_agent(text: &str) -> (Option<&str>, &str) {
    match text.strip_prefix('[').and_then(|t| t.split_once("] ")) {
        Some((name, rest)) if !name.is_empty() && name.len() < 80 && !name.contains(' ') => {
            (Some(name), rest)
        }
        _ => (None, text),
    }
}

/// Counts one line of a background agent.
pub(super) fn note_line(
    agents: &mut BTreeMap<String, BackgroundAgent>,
    name: &str,
    at: u64,
    stream: &str,
    text: &str,
) {
    let agent = agents
        .entry(name.to_owned())
        .or_insert_with(|| BackgroundAgent {
            name: name.to_owned(),
            lines: 0,
            first_at: at,
            last_at: at,
            last_line: String::new(),
            retries: 0,
            errors: 0,
            outcome: None,
            note: None,
        });
    agent.lines += 1;
    agent.last_at = at;
    if is_retry_line(text) {
        agent.retries += 1;
    }
    if stream == "error" {
        agent.errors += 1;
    }
}

/// Each background agent's last words, and its outcome from the run's own
/// `Active Memory <name> failed|completed` lines.
pub(super) fn settle(agents: &mut BTreeMap<String, BackgroundAgent>, run_log: &[LogLine]) {
    for line in run_log {
        match &line.agent {
            Some(name) => {
                if let Some(agent) = agents.get_mut(name) {
                    agent.last_line = line.text.trim().chars().take(300).collect();
                }
            }
            None => {
                let Some(rest) = line.text.strip_prefix(ACTIVE_MEMORY_PREFIX) else {
                    continue;
                };
                let Some((name, tail)) = rest.split_once(' ') else {
                    continue;
                };
                let Some(agent) = agents.get_mut(name) else {
                    continue;
                };
                match outcome(tail) {
                    Some(Outcome::Failed) => agent.outcome = Some("failed"),
                    Some(Outcome::Completed) => {
                        agent.outcome.get_or_insert("completed");
                    }
                    Some(Outcome::Note) => agent.note = Some(tail.chars().take(300).collect()),
                    None => {}
                }
            }
        }
    }
}

/// What an `Active Memory <name> <tail>` line says about the agent.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// It failed (this wins over an earlier `completed`).
    Failed,
    /// It completed (unless it also failed).
    Completed,
    /// Neither, but worth keeping as why it failed (`findings rejected: …`).
    Note,
}

/// The tail is `failed`, or starts with one of these (`failed: …`).
const FAILED: [&str; 2] = ["failed:", "failed "];
/// The tail is one of these, or starts with `completed`.
const COMPLETED: [&str; 2] = ["completed", "succeeded"];
/// A tail containing one of these is a note.
const NOTE_WORDS: [&str; 2] = ["rejected", "error"];

fn outcome(tail: &str) -> Option<Outcome> {
    if tail == "failed" || FAILED.iter().any(|p| tail.starts_with(p)) {
        Some(Outcome::Failed)
    } else if COMPLETED.contains(&tail) || tail.starts_with("completed") {
        Some(Outcome::Completed)
    } else if NOTE_WORDS.iter().any(|w| tail.contains(w)) {
        Some(Outcome::Note)
    } else {
        None
    }
}
