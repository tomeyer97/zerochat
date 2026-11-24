//! What the transcript readers share: tool results and their size limit, the provider texts
//! that mark an error, Zeroshot's own wrappers around the messages it sends, and which native
//! session belongs to an execution.

use serde_json::Value;

use super::{Item, ToolResult};

/// Tool results longer than this are cut (the rest is counted, not shown).
const MAX_TOOL_RESULT: usize = 64 * 1024;

/// A provider's own error text, matched anywhere in a line the agent printed or a message the
/// model sent.
const PROVIDER_ERRORS: [&str; 7] = [
    "There's an issue with the selected model",
    "Failed to authenticate",
    "API Error:",
    "Invalid API key",
    "Credit balance is too low",
    "You've hit your",
    "Please run /login",
];

/// A provider's own error text printed on the agent's output stream.
pub(super) fn is_provider_error(line: &str) -> bool {
    PROVIDER_ERRORS.iter().any(|p| line.contains(p))
}

/// A model message this short that carries a provider's error text is the error, not an answer
/// that quotes one.
pub(super) const MAX_ERROR_MESSAGE: usize = 600;

/// Zeroshot's own follow-up turns carry an internal tag ("[structured-output-enforce] You MUST
/// call …"); the instruction is real, the bracketed engine tag is not something to show.
pub(super) fn strip_auto_tag(text: &str) -> &str {
    text.strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
        .map_or(text, |(_, rest)| rest)
}

/// The user's text when `text` is a steer as Zeroshot delivers it: a header line starting
/// "Message from the user, sent while you were working" (its wording varies by version), then
/// the message.
pub fn steer_body(text: &str) -> Option<&str> {
    text.strip_prefix("Message from the user, sent while you were working")
        .and_then(|rest| rest.split_once('\n'))
        .map(|(_, body)| body)
}

/// A tool call item from a JSON object with the call's `id`, `name` and `input` (Claude's
/// `tool_use` blocks, and the ledger's classified tool lines).
pub(super) fn tool_call(at: Option<u64>, call: &Value) -> Item {
    Item::tool(
        at,
        call["id"].as_str().unwrap_or(""),
        call["name"].as_str().unwrap_or("tool"),
        call["input"].clone(),
    )
}

/// Attaches `result` to the latest call `id` that has none yet (to the latest open call when the
/// provider gives no id).
pub(super) fn attach_result(items: &mut [Item], id: &str, result: ToolResult) {
    let target = items.iter_mut().rev().find(|it| {
        it.tool
            .as_ref()
            .is_some_and(|t| (t.id == id || id.is_empty()) && t.result.is_none())
    });
    if let Some(it) = target {
        it.tool.as_mut().unwrap().result = Some(result);
    }
}

/// A tool result's content as text: text parts joined, images as `[image]`, cut at
/// `MAX_TOOL_RESULT`.
pub(super) fn content_text(value: &Value) -> String {
    let text = match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|p| match p["type"].as_str() {
                Some("text") => p["text"].as_str().unwrap_or("").to_owned(),
                Some("image") => "[image]".to_owned(),
                _ => p.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    if text.len() > MAX_TOOL_RESULT {
        let shown = crate::text::truncate_utf8(&text, MAX_TOOL_RESULT);
        format!(
            "{shown}\n… [{} more bytes not shown]",
            text.len() - shown.len()
        )
    } else {
        text
    }
}

// ---------- which session is the execution's ----------

/// A session without a recorded id is the execution's when its first record is at most
/// `EARLY_SLACK_MS` before the execution started, at most `LATE_SLACK_MS` after it ended, and at
/// most `MAX_START_DELAY_MS` after it started.
const EARLY_SLACK_MS: u64 = 30_000;
const LATE_SLACK_MS: u64 = 5_000;
const MAX_START_DELAY_MS: u64 = 180_000;

/// Whether a session whose first record is at `first` started inside the window of an
/// execution that ran from `start` to `end` (`u64::MAX` while it runs).
pub(super) fn started_in_window(first: u64, start: u64, end: u64) -> bool {
    first + EARLY_SLACK_MS >= start
        && first <= end.saturating_add(LATE_SLACK_MS)
        && first <= start + MAX_START_DELAY_MS
}

/// The one session found; none when there are several (ambiguous) or none.
pub(super) fn single<T>(mut found: Vec<T>) -> Option<T> {
    (found.len() == 1).then(|| found.pop()).flatten()
}
