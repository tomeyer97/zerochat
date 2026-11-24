//! A node execution's conversation as chat items, from the richest source available:
//! the provider's native session log (full prompt, reasoning, tool calls and results), else
//! the ledger's `safe_log` lines (messages, reasoning and tool calls) and the messages people
//! sent the agents (Zeroshot 16.1 `vector` steers and their receipts).
//!
//! This file is the chat model ([`Transcript`], [`Item`]). The readers sit side by side:
//!
//! - `ledger.rs`: the ledger's log lines and steers, for every provider;
//! - `claude.rs`: Claude Code's session logs (`~/.claude/projects/…/<session>.jsonl`);
//! - `copilot.rs`: GitHub Copilot CLI's session logs (`session-state/<session>/events.jsonl`);
//! - `session.rs`: what the native readers share (tool results, provider errors, which session
//!   belongs to an execution), and `iso_time.rs` for the timestamps in those logs.

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

mod claude;
mod copilot;
mod iso_time;
mod ledger;
mod session;

pub use claude::{find_claude_session_of, from_claude_session, usage_from_claude_session};
pub use copilot::{copilot_home, find_copilot_session, from_copilot_session};
pub use iso_time::parse_iso_millis;
pub use ledger::{
    ToolLine, from_ledger, merge_native_with_ledger, steer_receipts, steer_texts, tool_line,
};

// The goldens name these by this path.
#[cfg(test)]
pub use claude::claude_project_slug;
#[cfg(test)]
pub(crate) use ledger::tests::cut_tool_lines;
#[cfg(test)]
pub use session::steer_body;

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub kind: ItemKind,
    pub at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
    /// Longer text behind the one-line `text` (a compaction's summary).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tool: Option<Tool>,
    /// Set for messages of a sub-agent (Claude Task tool) rather than the node's agent.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<_>", optional)]
    pub subagent: bool,
}

impl Item {
    /// A message of any kind but a tool call, with its text.
    pub(crate) fn message(kind: ItemKind, at: Option<u64>, text: impl Into<String>) -> Item {
        Item {
            kind,
            at,
            text: Some(text.into()),
            detail: None,
            tool: None,
            subagent: false,
        }
    }

    /// A tool call, without its result yet.
    pub(crate) fn tool(at: Option<u64>, id: &str, name: &str, input: Value) -> Item {
        let tool = Tool {
            id: id.to_owned(),
            name: name.to_owned(),
            input,
            input_cut: false,
            result: None,
            permission: None,
        };
        Item {
            kind: ItemKind::Tool,
            at,
            text: None,
            detail: None,
            tool: Some(tool),
            subagent: false,
        }
    }

    /// The same item, said by a sub-agent when `subagent` is set.
    pub(crate) fn by_subagent(self, subagent: bool) -> Item {
        Item { subagent, ..self }
    }

    /// The same item with a longer text behind its one line.
    pub(crate) fn with_detail(self, detail: Option<String>) -> Item {
        Item { detail, ..self }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// The task the agent was given.
    Prompt,
    Text,
    Thinking,
    Tool,
    /// A notice from the harness, not the model.
    System,
    /// A message Zeroshot sent the agent by itself, e.g. a structured-output nudge.
    Auto,
    Error,
    /// A message a person sent the agent.
    Steer,
    Retry,
    RateLimit,
    /// The provider summarised the conversation to free context.
    Compaction,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub id: String,
    pub name: String,
    #[ts(type = "Json")]
    pub input: Value,
    /// Zeroshot cut the input in its log: `input` holds only the fields that could still be read
    /// (a command, a path, a pattern …, possibly cut short themselves).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<_>", optional)]
    pub input_cut: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub result: Option<ToolResult>,
    /// The permission the harness asked for before running it, and the answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permission: Option<Permission>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    /// shell | read | write | url | mcp | …
    pub kind: String,
    /// approved | denied… (as the provider says it); absent while pending
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub result: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub is_error: bool,
    pub content: String,
    pub at: Option<u64>,
    /// Exit status of a shell command, when the provider reports it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exit_code: Option<i64>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    /// native | ledger
    #[ts(type = "'native' | 'ledger'")]
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_path: Option<String>,
    /// What this source cannot show, in plain words.
    pub limitations: Vec<String>,
    pub items: Vec<Item>,
    /// Copilot's own billing figure for the session: the sum of its per-call `totalNanoAiu`, in
    /// AI credits (1 credit = $0.01 by Copilot's schema). Not the run's $ estimate.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub copilot_credits: Option<f64>,
}

/// The types of this module the web reads (`web/src/generated/transcript.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, transcript: ItemKind, Item, Tool, Permission, ToolResult, Transcript)
}
