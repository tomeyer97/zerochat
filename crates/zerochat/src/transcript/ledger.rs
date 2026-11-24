//! The transcript from the ledger: one execution's `safe_log` lines (messages, reasoning, tool
//! calls, Zeroshot's retry and rate-limit notices) and the steers people sent it. Every
//! provider's runs have it; a native session log, when there is one, shows more.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::session::{attach_result, content_text, is_provider_error, tool_call};
use super::{Item, ItemKind, ToolResult, Transcript};
use crate::engine_lines::{humanize_retry, is_retry_line, parse_rate_limit_line};
use crate::projection::PressureLevel;
use crate::source::Event;

/// Deltas of one text or reasoning block arrive closer together than this; a longer gap starts a
/// new block.
const DELTA_GAP_MS: u64 = 1500;

/// Unclassified lines (older ledgers) that start like this are Zeroshot's rate-limit notices.
const RATE_LIMIT_NOTICE: &str = "Claude rate limit:";

/// A rate-limit notice that says the limit is fine: nothing to show.
const RATE_LIMIT_ALLOWED: &str = "Claude rate limit: status=allowed,";

/// Unclassified lines that start like this are Zeroshot's own notices (shown as `system`), not
/// the model's reasoning.
const NOTICE_PREFIXES: [&str; 10] = [
    "Claude rate limit",
    "Claude API retry",
    "Claude provider",
    "Preparing",
    "Node ",
    "Codex ",
    "Copilot ",
    "Zeroshot",
    "Starting",
    "Resuming",
];

fn is_notice(line: &str) -> bool {
    NOTICE_PREFIXES.iter().any(|p| line.starts_with(p))
}

/// The item kinds only the ledger has (Zeroshot's retry and rate-limit notices, and the steers
/// with their delivery status) and that a native transcript takes from it.
const LEDGER_ONLY_KINDS: [ItemKind; 3] = [ItemKind::Retry, ItemKind::RateLimit, ItemKind::Steer];

/// Builds the transcript from ledger lines of one execution.
pub fn from_ledger(events: &[Event], execution: u64) -> Transcript {
    let mut items: Vec<Item> = Vec::new();
    let steers = Steers::of(events, execution);
    let mut lines = 0usize;
    let mut last_at = 0u64;
    for event in events {
        let body = &event.body;
        if body["kind"] == "vector" {
            if let Some(it) = steers.item(event) {
                last_at = it.at.unwrap_or(last_at);
                items.push(it);
            }
            continue;
        }
        if body["kind"] != "safe_log" || body["execution"].as_u64() != Some(execution) {
            continue;
        }
        lines += 1;
        let at = body["timestamp"].as_u64().unwrap_or(0);
        let line = body["line"].as_str().unwrap_or("");
        match classify(
            body["logKind"].as_str(),
            body["stream"].as_str().unwrap_or("output"),
            line,
            at,
        ) {
            Line::Skip => continue,
            Line::Quiet => {}
            Line::Item(item) => items.push(item),
            Line::Result { id, result } => attach_result(&mut items, &id, result),
            Line::Text(kind) => push_text(&mut items, kind, at, last_at, line),
        }
        last_at = at;
    }
    let mut limitations = Vec::new();
    if !records_tool_calls(events) && lines > 0 {
        limitations.push("Tool calls not recorded by this Zeroshot".to_owned());
    }
    Transcript {
        source: "ledger",
        source_path: None,
        limitations,
        items,
        copilot_credits: None,
    }
}

/// Whether this Zeroshot records tool calls in the ledger. That is a property of the run, not
/// of one execution: a reused or silent execution has no lines of its own. Zeroshot 16.1
/// writes tool calls as plain lines ("Claude tool call: Bash id=… status=…"); the older
/// patched Zeroshot classified every line (`logKind`).
fn records_tool_calls(events: &[Event]) -> bool {
    events.iter().any(|e| {
        let line = e.body["line"].as_str();
        e.body["kind"] == "safe_log"
            && (e.body["logKind"].is_string() || line.is_some_and(|l| tool_line(l).is_some()))
    })
}

/// What one log line of the execution becomes in the chat.
enum Line {
    /// Text or reasoning (or an error, a notice) shown as `kind`; consecutive deltas of one
    /// block are joined.
    Text(ItemKind),
    /// An item of its own: a tool call, a steer, a retry or a rate-limit notice.
    Item(Item),
    /// The result of the tool call `id`.
    Result { id: String, result: ToolResult },
    /// Nothing to show, but the line still counts as the latest one (a routine rate-limit
    /// notice in the classified form).
    Quiet,
    /// Nothing to show.
    Skip,
}

/// `log_kind`: the line's class in ledgers that classify lines (`tool_use`, `thinking`, …);
/// `None` in ledgers that do not, where the stream and the text tell.
fn classify(log_kind: Option<&str>, stream: &str, line: &str, at: u64) -> Line {
    match (log_kind, stream) {
        (Some("tool_use"), _) => {
            let call: Value =
                serde_json::from_str(line).unwrap_or(json!({"name": "tool", "input": line}));
            match (&call["truncated"], &call["input"]) {
                (Value::Bool(true), Value::String(cut)) => Line::Item(cut_call(
                    at,
                    call["id"].as_str().unwrap_or(""),
                    call["name"].as_str().unwrap_or("tool"),
                    &[cut],
                )),
                _ => Line::Item(tool_call(Some(at), &call)),
            }
        }
        (Some("tool_result"), _) => {
            let v: Value = serde_json::from_str(line).unwrap_or(json!({"content": line}));
            let result = ToolResult {
                is_error: v["isError"].as_bool().unwrap_or(false),
                content: content_text(&v["content"]),
                at: Some(at),
                exit_code: v["exitCode"].as_i64(),
            };
            Line::Result {
                id: v["toolUseId"].as_str().unwrap_or("").to_owned(),
                result,
            }
        }
        (Some("session"), _) => Line::Skip,
        (Some("thinking"), _) => Line::Text(ItemKind::Thinking),
        (Some("text"), _) => Line::Text(ItemKind::Text),
        (Some("steer"), _) => Line::Item(steer_line(at, line)),
        (Some("retry"), _) => Line::Item(retry(at, line)),
        (Some("rate_limit"), _) => rate_limit(at, line),
        (None, _) if tool_line(line).is_some() => tool_line_of(at, line),
        (_, "error") => Line::Text(ItemKind::Error),
        (_, _) if line.starts_with(RATE_LIMIT_ALLOWED) => Line::Skip,
        (_, _) if line.starts_with(RATE_LIMIT_NOTICE) => rate_limit(at, line),
        (_, _) if is_retry_line(line) => Line::Item(retry(at, line)),
        (None, "output") if is_provider_error(line) => Line::Text(ItemKind::Error),
        (None, "output") => Line::Text(ItemKind::Text),
        (None, _) if is_notice(line) => Line::Text(ItemKind::System),
        (None, _) => Line::Text(ItemKind::Thinking),
        (Some(_), _) => Line::Text(ItemKind::System),
    }
}

/// Providers stream text and reasoning as deltas: a delta that follows one of the same block
/// is appended to it, anything else is an item of its own.
fn push_text(items: &mut Vec<Item>, kind: ItemKind, at: u64, last_at: u64, line: &str) {
    if matches!(kind, ItemKind::Text | ItemKind::Thinking)
        && let Some(last) = items.last_mut()
        && last.kind == kind
        && at.saturating_sub(last_at) < DELTA_GAP_MS
    {
        last.text.get_or_insert_with(String::new).push_str(line);
        return;
    }
    items.push(Item::message(kind, Some(at), line));
}

/// A classified steer line: `{message, status}`; a status other than `delivered` is shown as a
/// trailing `[status]`.
fn steer_line(at: u64, line: &str) -> Item {
    let v: Value = serde_json::from_str(line).unwrap_or(json!({"message": line}));
    let status = v["status"].as_str().unwrap_or("");
    let mut it = Item::message(
        ItemKind::Steer,
        Some(at),
        v["message"].as_str().unwrap_or(line),
    );
    if !status.is_empty() && status != "delivered" {
        it.text = Some(format!("{}\n\n[{status}]", it.text.unwrap_or_default()));
    }
    it
}

/// An API retry, in plain words (`retried 2 of 10 (rate limit)`), classified or not.
fn retry(at: u64, line: &str) -> Item {
    Item::message(
        ItemKind::Retry,
        Some(at),
        humanize_retry(line).unwrap_or_else(|| line.to_owned()),
    )
}

/// A rate-limit notice in plain words (`7-day limit: 93% used`), classified or not. Routine
/// notices (the limit is fine) say nothing.
fn rate_limit(at: u64, line: &str) -> Line {
    match parse_rate_limit_line(line) {
        Some(parsed) if parsed.level == PressureLevel::Info => Line::Quiet,
        Some(parsed) => Line::Item(Item::message(ItemKind::RateLimit, Some(at), parsed.text)),
        None => Line::Item(Item::message(ItemKind::RateLimit, Some(at), line)),
    }
}

/// A Zeroshot 16.1 tool line: the call, or the result for its call.
fn tool_line_of(at: u64, line: &str) -> Line {
    match tool_line(line) {
        Some(ToolLine::Call { id, name, input }) => Line::Item(match serde_json::from_str(input) {
            Ok(value) => Item::tool(Some(at), id, name, value),
            Err(_) => match input.split_once(CUT_MARK) {
                Some((head, tail)) => cut_call(at, id, name, &[head, tail]),
                None => Item::tool(Some(at), id, name, json!(input)),
            },
        }),
        Some(ToolLine::Completed {
            id,
            status,
            content,
        }) => {
            let is_error = matches!(status, "error" | "failed" | "failure");
            let result = ToolResult {
                is_error,
                content: content_text(&json!(content)),
                at: Some(at),
                exit_code: None,
            };
            Line::Result {
                id: id.to_owned(),
                result,
            }
        }
        None => Line::Skip,
    }
}

/// Where Zeroshot 16.1 cut a long tool line: it keeps the head and the tail of the payload,
/// with this between them (`tool_activity.rs` `excerpt`).
const CUT_MARK: &str = "\n...[truncated]...\n";

/// The input fields worth reading back from a cut call: what its one-line summary shows (the
/// command, the file, the pattern, the query, the URL …), not the bulk (file contents, edits).
const READABLE_FIELDS: [&str; 9] = [
    "command",
    "description",
    "file_path",
    "path",
    "notebook_path",
    "pattern",
    "query",
    "url",
    "prompt",
];

/// A tool call whose input Zeroshot cut in its log (the JSON no longer parses): its name, the
/// `READABLE_FIELDS` that can still be read from the `parts` that are left, and the cut mark.
fn cut_call(at: u64, id: &str, name: &str, parts: &[&str]) -> Item {
    let mut input = serde_json::Map::new();
    for key in READABLE_FIELDS {
        if let Some(value) = parts.iter().find_map(|part| cut_string_field(part, key)) {
            input.insert(key.to_owned(), Value::String(value));
        }
    }
    let mut item = Item::tool(Some(at), id, name, Value::Object(input));
    if let Some(tool) = item.tool.as_mut() {
        tool.input_cut = true;
    }
    item
}

/// The string value of `"key":"…"` in a JSON text cut anywhere: up to its closing quote, or what
/// is left of it when the cut falls inside it. `None` when the key is not there.
fn cut_string_field(text: &str, key: &str) -> Option<String> {
    let start = text.find(&format!("\"{key}\":"))? + key.len() + 3;
    let rest = text[start..].trim_start().strip_prefix('"')?;
    let mut end = rest.len();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => {
                end = i;
                break;
            }
            '\\' => {
                let escape = chars.next().map(|(_, e)| e);
                let complete = match escape {
                    Some('u') => {
                        (0..4).all(|_| chars.next().is_some_and(|(_, h)| h.is_ascii_hexdigit()))
                    }
                    Some(_) => true,
                    None => false,
                };
                if !complete {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    serde_json::from_str::<String>(&format!("\"{}\"", &rest[..end])).ok()
}

/// A tool line as Zeroshot 16.1 writes it for every harness: a header
/// `<Harness> tool call: NAME id=ID status=STATUS` or `<Harness> tool completed: …`, then the
/// call's JSON input or the result's text on the following lines.
#[derive(Debug, PartialEq)]
pub enum ToolLine<'a> {
    Call {
        id: &'a str,
        name: &'a str,
        input: &'a str,
    },
    Completed {
        id: &'a str,
        status: &'a str,
        content: &'a str,
    },
}

pub fn tool_line(line: &str) -> Option<ToolLine<'_>> {
    let (header, rest) = line.split_once('\n').unwrap_or((line, ""));
    let (harness, completed, fields) = if let Some((h, f)) = header.split_once(" tool call: ") {
        (h, false, f)
    } else {
        let (h, f) = header.split_once(" tool completed: ")?;
        (h, true, f)
    };
    if !["Claude", "Codex", "Copilot", "Pi", "OpenCode"].contains(&harness) {
        return None;
    }
    let mut parts = fields.split(' ');
    let name = parts.next().filter(|n| !n.is_empty())?;
    let mut id = "";
    let mut status = "";
    for part in parts {
        if let Some(v) = part.strip_prefix("id=") {
            id = v;
        } else if let Some(v) = part.strip_prefix("status=") {
            status = v;
        }
    }
    Some(if completed {
        ToolLine::Completed {
            id,
            status,
            content: rest,
        }
    } else {
        ToolLine::Call {
            id,
            name,
            input: rest,
        }
    })
}

/// Zeroshot 16.1 steers that concern one execution: `vector` events of kind `steer` (a person's
/// message: to one node with `node`, else to every agent of the run) and `steer_receipt` (an
/// agent conversation took it in, see [`steer_receipts`]). Agent-to-agent
/// network messages (`network`) are not a person's and are left out.
struct Steers {
    node: Option<String>,
    /// Sequence of the execution's `node_started`, and of its end (`node_completed`/voided).
    started: Option<u64>,
    ended: Option<u64>,
    /// Steer id → when this execution received it.
    received: HashMap<String, u64>,
}

impl Steers {
    fn of(events: &[Event], execution: u64) -> Self {
        let mut steers = Steers {
            node: None,
            started: None,
            ended: None,
            received: Default::default(),
        };
        let own = |reference: &Value| reference["execution"].as_u64() == Some(execution);
        let receipts = steer_receipts(events);
        for event in events {
            let body = &event.body;
            match body["kind"].as_str().unwrap_or("") {
                "node_started" if own(&body["reference"]) => {
                    steers.node = body["reference"]["node"].as_str().map(str::to_owned);
                    steers.started = Some(event.seq);
                }
                "node_completed" if own(&body["completion"]["reference"]) => {
                    steers.ended = Some(event.seq)
                }
                "execution_voided" if own(&body["reference"]) => steers.ended = Some(event.seq),
                "vector" if body["vector"]["kind"] == "steer_receipt" => {
                    let v = &body["vector"];
                    let to_this = receipts.get(&event.seq) == Some(&execution);
                    if let (true, Some(id)) = (to_this, v["steerId"].as_str()) {
                        steers
                            .received
                            .insert(id.to_owned(), v["createdAt"].as_u64().unwrap_or(0));
                    }
                }
                _ => {}
            }
        }
        steers
    }

    /// The chat item for a `steer` vector, when it concerns this execution: received by it, or
    /// sent to it (or to every agent) while it was running. The text ends in a `[tag]` the web
    /// shows as its status: `received`, `sent` (not yet), `not received`; `, to all agents` when
    /// the message went to the whole run.
    fn item(&self, event: &Event) -> Option<Item> {
        let v = &event.body["vector"];
        if v["kind"] != "steer" || !v["network"].is_null() {
            return None;
        }
        let id = v["id"].as_str().unwrap_or("");
        let target = v["node"].as_str();
        let received = self.received.contains_key(id);
        let for_this = target.is_none() || target == self.node.as_deref();
        let while_running =
            self.started.is_some_and(|s| event.seq > s) && self.ended.is_none_or(|e| event.seq < e);
        if !(received || (for_this && while_running)) {
            return None;
        }
        let state = if received {
            "received"
        } else if self.ended.is_none() {
            "sent"
        } else {
            "not received"
        };
        let scope = if target.is_none() {
            ", to all agents"
        } else {
            ""
        };
        let at = v["createdAt"]
            .as_u64()
            .or_else(|| event.body["at"].as_u64());
        Some(Item::message(
            ItemKind::Steer,
            at,
            format!("{}\n\n[{state}{scope}]", v["text"].as_str().unwrap_or("")),
        ))
    }
}

/// Zeroshot's public execution ref (`nv2-<sha256>`), as receipts, `status` and `logs` name an
/// execution: SHA-256 of `zeroshot/native-v2/execution-ref/v1\0<run>\0<node>\0<instance>\0<execution>`
/// (Zeroshot's `ExecutionRef::public_execution`).
pub fn public_execution_ref(
    run_id: &str,
    node: &str,
    node_instance: u64,
    execution: u64,
) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"zeroshot/native-v2/execution-ref/v1\0");
    digest.update(format!("{run_id}\0{node}\0{node_instance}\0{execution}").as_bytes());
    digest
        .finalize()
        .iter()
        .fold(String::from("nv2-"), |mut out, byte| {
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Which execution took in each steer: `steer_receipt` event sequence → execution id.
///
/// A receipt names the execution by its public ref (`execution: "nv2-…"`, see
/// [`public_execution_ref`]), matched exactly against every `node_started` reference. Its
/// `conversation` (`<run>/execution/<n>`) counts conversations, not zerochat's execution ids (a
/// reviewer running as execution 3 got `execution/2`), so it is not used. A receipt whose ref
/// matches nothing falls back to the execution of its `nodeInstance` that started last before it.
pub fn steer_receipts(events: &[Event]) -> HashMap<u64, u64> {
    let mut by_ref: HashMap<String, u64> = HashMap::new();
    for event in events {
        let r = &event.body["reference"];
        if event.body["kind"] == "node_started"
            && let (Some(run), Some(node), Some(instance), Some(execution)) = (
                r["runId"].as_str(),
                r["node"].as_str(),
                r["nodeInstance"].as_u64(),
                r["execution"].as_u64(),
            )
        {
            by_ref.insert(
                public_execution_ref(run, node, instance, execution),
                execution,
            );
        }
    }
    let mut latest: HashMap<u64, u64> = HashMap::new();
    let mut out = HashMap::new();
    for event in events {
        let body = &event.body;
        match body["kind"].as_str().unwrap_or("") {
            "node_started" => {
                if let (Some(instance), Some(execution)) = (
                    body["reference"]["nodeInstance"].as_u64(),
                    body["reference"]["execution"].as_u64(),
                ) {
                    latest.insert(instance, execution);
                }
            }
            "vector" if body["vector"]["kind"] == "steer_receipt" => {
                let v = &body["vector"];
                let exact = v["execution"].as_str().and_then(|r| by_ref.get(r).copied());
                let by_instance = || {
                    v["nodeInstance"]
                        .as_u64()
                        .and_then(|i| latest.get(&i).copied())
                };
                if let Some(execution) = exact.or_else(by_instance) {
                    out.insert(event.seq, execution);
                }
            }
            _ => {}
        }
    }
    out
}

/// The messages people sent this execution, as the transcript shows them (without the trailing
/// `[status]`), to recognise them among a native session log's user messages.
pub fn steer_texts(transcript: &Transcript) -> Vec<String> {
    transcript
        .items
        .iter()
        .filter(|i| i.kind == ItemKind::Steer)
        .filter_map(|i| i.text.as_deref())
        .map(|t| t.split("\n\n[").next().unwrap_or(t).trim().to_owned())
        .collect()
}

/// Native sessions never carry the ledger's retry/rate-limit system lines (they are Zeroshot's
/// own, not Claude's): merge them in so a node's chat shows what it survived. Steer messages
/// are replaced by the ledger's own copies, which carry the delivery status
/// ("queued"/"rejected"/…); the native source only guesses "steer" by matching text, with no
/// status, and misses it entirely when the turn never got far enough to echo it back (a reload
/// must not lose a sent message).
pub fn merge_native_with_ledger(native: &mut Transcript, ledger_items: Vec<Item>) {
    native.items.retain(|i| i.kind != ItemKind::Steer);
    let mut extra: Vec<Item> = ledger_items
        .into_iter()
        .filter(|i| LEDGER_ONLY_KINDS.contains(&i.kind))
        .collect();
    if extra.is_empty() {
        return;
    }
    native.items.append(&mut extra);
    native.items.sort_by_key(|i| i.at.unwrap_or(0));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::transcript::Tool;

    fn log(seq: u64, at: u64, stream: &str, line: &str) -> Event {
        Event {
            seq,
            body: json!({"kind":"safe_log","execution":1,"timestamp":at,"stream":stream,"line":line}),
        }
    }

    #[test]
    fn joins_deltas_and_separates_blocks() {
        let events = vec![
            log(1, 1000, "system", "thinking "),
            log(2, 1100, "system", "more"),
            log(3, 1200, "output", "Hello "),
            log(4, 1300, "output", "world"),
            log(5, 9000, "output", "Later block"),
            log(
                6,
                9100,
                "system",
                "Claude rate limit: status=allowed, type=five_hour, resetsAt=x",
            ),
        ];
        let t = from_ledger(&events, 1);
        let kinds: Vec<_> = t.items.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [ItemKind::Thinking, ItemKind::Text, ItemKind::Text],
            "the allowed rate-limit status line is dropped"
        );
        assert_eq!(t.items[0].text.as_deref(), Some("thinking more"));
        assert_eq!(t.items[1].text.as_deref(), Some("Hello world"));
        assert_eq!(t.limitations.len(), 1);
    }

    #[test]
    fn retry_and_rate_limit_lines_are_humanised_in_the_chat() {
        // The raw engine text does not reach the node's chat.
        let events = vec![
            log(1, 1000, "system", "Claude API retry 2/10: rate_limit"),
            log(
                2,
                2000,
                "system",
                "Claude rate limit: status=allowed_warning, type=seven_day, utilization=93%, resetsAt=2026-11-11T02:00:00Z",
            ),
            log(
                3,
                3000,
                "system",
                "Claude rate limit: status=allowed_warning, type=seven_day, utilization=33%, resetsAt=2026-11-11T02:00:00Z",
            ),
        ];
        let t = from_ledger(&events, 1);
        assert_eq!(t.items[0].kind, ItemKind::Retry);
        assert_eq!(
            t.items[0].text.as_deref(),
            Some("retried 2 of 10 (rate limit)")
        );
        assert_eq!(t.items[1].kind, ItemKind::RateLimit);
        assert_eq!(t.items[1].text.as_deref(), Some("7-day limit: 93% used"));
        // The 33% line is below the real threshold, so it says nothing (same as "allowed").
        assert_eq!(t.items.len(), 2);
    }

    #[test]
    fn merge_replaces_native_steer_tags_and_adds_retry_lines() {
        // The native source's own "steer" guess (no status, and it can miss the
        // message entirely) is replaced by the ledger's copy, which carries the delivery status.
        let mut native = Transcript {
            source: "native",
            source_path: None,
            limitations: Vec::new(),
            items: vec![
                Item::message(ItemKind::Text, Some(500), "Hello"),
                Item::message(ItemKind::Steer, Some(1000), "Also check logging"),
            ],
            copilot_credits: None,
        };
        let ledger_items = vec![
            Item::message(
                ItemKind::Steer,
                Some(1000),
                "Also check logging\n\n[queued]",
            ),
            Item::message(ItemKind::Retry, Some(1200), "retried 1 of 10 (rate limit)"),
        ];
        merge_native_with_ledger(&mut native, ledger_items);
        let kinds: Vec<_> = native
            .items
            .iter()
            .map(|i| (i.kind, i.text.as_deref()))
            .collect();
        assert_eq!(
            kinds,
            [
                (ItemKind::Text, Some("Hello")),
                (ItemKind::Steer, Some("Also check logging\n\n[queued]")),
                (ItemKind::Retry, Some("retried 1 of 10 (rate limit)")),
            ]
        );
    }

    #[test]
    fn tool_call_caveat_only_when_the_run_records_no_classified_lines() {
        let tool = Event {
            seq: 1,
            body: json!({"kind":"safe_log","execution":1,"timestamp":1,"stream":"system","logKind":"tool_use","line":"{\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}"}),
        };
        let mut plain = log(2, 2, "output", "hi");
        plain.body["execution"] = json!(2);
        // Execution 2 has only unclassified lines, but this Zeroshot does record tool calls.
        let t = from_ledger(&[tool.clone(), plain.clone()], 2);
        assert!(t.limitations.is_empty());
        // A reused execution with no lines of its own claims nothing either.
        let t = from_ledger(&[plain], 3);
        assert!(t.limitations.is_empty());
        let t = from_ledger(&[tool], 1);
        assert_eq!(t.items.len(), 1);
        assert!(t.limitations.is_empty());
    }

    #[test]
    fn copilot_ledger_tool_results_keep_the_exit_code() {
        let line = |kind: &str, line: &str| Event {
            seq: 1,
            body: json!({"kind":"safe_log","execution":1,"timestamp":1,"stream":"system","logKind":kind,"line":line}),
        };
        let t = from_ledger(
            &[
                line(
                    "tool_use",
                    r#"{"id":"c1","input":{"command":"ls"},"name":"bash"}"#,
                ),
                line(
                    "tool_result",
                    r#"{"content":"x","exitCode":2,"isError":false,"toolUseId":"c1"}"#,
                ),
            ],
            1,
        );
        let result = t.items[0].tool.as_ref().unwrap().result.as_ref().unwrap();
        assert_eq!((result.is_error, result.exit_code), (false, Some(2)));
        assert!(t.limitations.is_empty());
    }

    fn ev(seq: u64, body: Value) -> Event {
        Event { seq, body }
    }

    /// Two agents; one run-wide steer (received by the worker only), one to the reviewer, one to
    /// the worker after it ended, and an agent-to-agent network note.
    fn sixteen_one_run() -> Vec<Event> {
        let started = |execution: u64, node: &str| json!({"kind": "node_started", "reference": {"node": node, "nodeInstance": 1, "execution": execution}, "occurrence": {"node": node, "mapIndices": []}, "attempt": 1, "input": {}});
        let completed = |execution: u64, node: &str| json!({"kind": "node_completed", "completion": {"reference": {"node": node, "nodeInstance": 1, "execution": execution}, "outcome": {"status": "verified"}}});
        let steer = |id: &str, node: Option<&str>, text: &str, at: u64| json!({"kind": "vector", "vector": {"id": id, "kind": "steer", "node": node, "text": text, "mode": "once", "createdAt": at}});
        let receipt = |steer: &str, execution: u64| json!({"kind": "vector", "vector": {"id": format!("r-{steer}-{execution}"), "kind": "steer_receipt", "steerId": steer, "conversation": format!("run/execution/{execution}"), "execution": "nv2-x", "nodeInstance": 1, "createdAt": 50}});
        vec![
            ev(1, started(1, "worker")),
            ev(
                2,
                json!({"kind": "safe_log", "execution": 1, "timestamp": 10, "stream": "system", "line": "Claude tool call: Bash id=t1 status=unknown\n{\"command\":\"ls\"}"}),
            ),
            ev(3, steer("all", None, "Everyone: use tabs.", 20)),
            ev(
                4,
                json!({"kind": "safe_log", "execution": 1, "timestamp": 30, "stream": "system", "line": "Claude tool completed: Bash id=t1 status=success\nREADME.md"}),
            ),
            ev(5, receipt("all", 1)),
            ev(
                6,
                json!({"kind": "safe_log", "execution": 1, "timestamp": 40, "stream": "system", "line": "Claude tool call: Read id=t2 status=unknown\n{\"file_path\":\"x\"}"}),
            ),
            ev(
                7,
                json!({"kind": "safe_log", "execution": 1, "timestamp": 41, "stream": "system", "line": "Claude tool completed: Read id=t2 status=error\nno such file"}),
            ),
            ev(8, completed(1, "worker")),
            ev(9, started(2, "review")),
            ev(10, steer("rev", Some("review"), "Reviewer: be strict.", 60)),
            ev(11, steer("late", Some("worker"), "Worker: too late.", 61)),
            ev(
                12,
                json!({"kind": "vector", "vector": {"id": "net", "kind": "steer", "text": "peer note", "mode": "once", "createdAt": 62, "network": {"networks": ["n"], "origin": {}}}}),
            ),
        ]
    }

    #[test]
    fn sixteen_one_tool_lines_become_tool_calls_with_results() {
        let t = from_ledger(&sixteen_one_run(), 1);
        let tools: Vec<&Tool> = t.items.iter().filter_map(|i| i.tool.as_ref()).collect();
        assert_eq!(tools.len(), 2);
        assert_eq!(
            (tools[0].name.as_str(), tools[0].input["command"].as_str()),
            ("Bash", Some("ls"))
        );
        assert_eq!(
            tools[0]
                .result
                .as_ref()
                .map(|r| (r.content.as_str(), r.is_error)),
            Some(("README.md", false))
        );
        assert_eq!(tools[1].result.as_ref().map(|r| r.is_error), Some(true));
        assert!(
            t.limitations.is_empty(),
            "16.1 records tool calls: {:?}",
            t.limitations
        );
        assert!(
            !t.items.iter().any(|i| i.kind == ItemKind::Thinking),
            "tool lines are not reasoning"
        );
        assert_eq!(tool_line("Claude rate limit: status=allowed"), None);
        assert_eq!(
            tool_line("Codex tool call: shell id=c1 status=unknown"),
            Some(ToolLine::Call {
                id: "c1",
                name: "shell",
                input: ""
            })
        );
    }

    /// Zeroshot 16.1 cuts a long payload in the middle (`CUT_MARK`); the patched build cuts the
    /// input to a string and says `"truncated": true`.
    pub(crate) fn cut_tool_lines() -> Vec<Event> {
        let line = |seq: u64, line: String| Event {
            seq,
            body: json!({"kind": "safe_log", "execution": 1, "timestamp": seq * 10, "stream": "system", "line": line}),
        };
        let classified = |seq: u64, kind: &str, line: Value| Event {
            seq,
            body: json!({"kind": "safe_log", "execution": 1, "timestamp": seq * 10, "stream": "system", "logKind": kind, "line": line.to_string()}),
        };
        let heredoc = format!(
            "cd /w && python3 - <<'EOF'\nprint(\"{}\")\nEOF",
            "x".repeat(40)
        );
        let bash = json!({"command": heredoc, "description": "Rewrite the file"}).to_string();
        let edit = json!({"file_path": "/w/src/a.ts", "new_string": "y".repeat(60), "old_string": "z".repeat(60)}).to_string();
        vec![
            line(
                1,
                format!(
                    "Claude tool call: Bash id=b1 status=unknown\n{}{CUT_MARK}{}",
                    &bash[..50],
                    &bash[bash.len() - 40..]
                ),
            ),
            line(
                2,
                "Claude tool completed: Bash id=b1 status=success\ndone".to_owned(),
            ),
            line(
                3,
                format!(
                    "Claude tool call: Edit id=e1 status=unknown\n{}{CUT_MARK}{}",
                    &edit[..40],
                    &edit[edit.len() - 10..]
                ),
            ),
            line(
                4,
                "Claude tool completed: Edit id=e1 status=error\nold_string not found".to_owned(),
            ),
            classified(
                5,
                "tool_use",
                json!({"id": "r1", "name": "Read", "input": r#"{"file_path":"/w/README.md","limit":4"#, "truncated": true}),
            ),
            classified(
                6,
                "tool_result",
                json!({"toolUseId": "r1", "content": "# Title", "isError": false}),
            ),
            line(
                7,
                "Claude tool call: Bash id=b2 status=unknown\n{\"command\":\"ls\"}".to_owned(),
            ),
        ]
    }

    #[test]
    fn a_cut_tool_call_keeps_its_name_what_can_be_read_and_its_result() {
        let t = from_ledger(&cut_tool_lines(), 1);
        let tools: Vec<&Tool> = t.items.iter().filter_map(|i| i.tool.as_ref()).collect();
        assert_eq!(tools.len(), 4, "every call is one tool item: {:?}", t.items);
        let bash = tools[0];
        assert_eq!((bash.name.as_str(), bash.input_cut), ("Bash", true));
        assert_eq!(
            bash.input,
            json!({"command": "cd /w && python3 - <<'EOF'\nprint(\"xx", "description": "Rewrite the file"}),
            "the command up to the cut, the description from the tail"
        );
        assert_eq!(
            bash.result
                .as_ref()
                .map(|r| (r.content.as_str(), r.is_error)),
            Some(("done", false))
        );
        let edit = tools[1];
        assert_eq!((edit.name.as_str(), edit.input_cut), ("Edit", true));
        assert_eq!(
            edit.input,
            json!({"file_path": "/w/src/a.ts"}),
            "the path, not a piece of the edit"
        );
        assert_eq!(edit.result.as_ref().map(|r| r.is_error), Some(true));
        let read = tools[2];
        assert_eq!(
            (read.name.as_str(), read.input_cut, &read.input),
            ("Read", true, &json!({"file_path": "/w/README.md"}))
        );
        assert_eq!(
            read.result.as_ref().map(|r| r.content.as_str()),
            Some("# Title")
        );
        // An uncut call is as before, and says nothing about cutting.
        assert_eq!(
            (tools[3].input_cut, &tools[3].input),
            (false, &json!({"command": "ls"}))
        );
        assert!(
            serde_json::to_value(tools[3])
                .unwrap()
                .get("inputCut")
                .is_none()
        );
        assert_eq!(serde_json::to_value(bash).unwrap()["inputCut"], json!(true));
    }

    #[test]
    fn a_field_cut_inside_an_escape_reads_up_to_it() {
        assert_eq!(
            cut_string_field(r#"{"command":"a\nb\"cé"}"#, "command").as_deref(),
            Some("a\nb\"cé")
        );
        assert_eq!(
            cut_string_field(r#"{"command":"echo \u00"#, "command").as_deref(),
            Some("echo ")
        );
        assert_eq!(
            cut_string_field(r#"{"command":"echo \"#, "command").as_deref(),
            Some("echo ")
        );
        assert_eq!(
            cut_string_field(r#"{"file_path":"/x","path":"/y"}"#, "path").as_deref(),
            Some("/y"),
            "not the tail of file_path"
        );
        assert_eq!(cut_string_field(r#"{"limit":4}"#, "path"), None);
        assert_eq!(cut_string_field(r#"{"path":null}"#, "path"), None);
        // A payload that is not JSON and has no cut mark stays the text it is.
        let t = from_ledger(
            &[log(
                1,
                1,
                "system",
                "Codex tool call: shell id=c1 status=unknown\nls -la",
            )],
            1,
        );
        let tool = t.items[0].tool.as_ref().unwrap();
        assert_eq!((tool.input_cut, &tool.input), (false, &json!("ls -la")));
    }

    #[test]
    fn sixteen_one_steers_show_per_execution_with_their_receipt() {
        let events = sixteen_one_run();
        let steers = |execution: u64| -> Vec<String> {
            from_ledger(&events, execution)
                .items
                .into_iter()
                .filter(|i| i.kind == ItemKind::Steer)
                .map(|i| i.text.unwrap())
                .collect()
        };
        // The worker received the run-wide message; the reviewer-only one is not its, and the one
        // sent to the worker after it ended never reached it (not this execution's).
        assert_eq!(
            steers(1),
            ["Everyone: use tabs.\n\n[received, to all agents]"]
        );
        // The reviewer is still running: its own message is sent, not received yet. The network
        // note is an agent's, not a person's.
        assert_eq!(steers(2), ["Reviewer: be strict.\n\n[sent]"]);
        assert_eq!(
            steer_texts(&from_ledger(&events, 2)),
            ["Reviewer: be strict."]
        );
    }

    #[test]
    fn a_steer_sent_while_running_but_never_taken_in_says_so() {
        let mut events = sixteen_one_run();
        events.push(ev(13, json!({"kind": "node_completed", "completion": {"reference": {"node": "review", "nodeInstance": 1, "execution": 2}, "outcome": {"status": "verified"}}})));
        let items = from_ledger(&events, 2).items;
        let steer = items.iter().find(|i| i.kind == ItemKind::Steer).unwrap();
        assert_eq!(
            steer.text.as_deref(),
            Some("Reviewer: be strict.\n\n[not received]")
        );
    }

    /// A demo run (16.1 with `steer --node`): six messages to the code reviewer, the acceptance
    /// reviewer and the worker in different rounds. Each receipt's `conversation` number is not the
    /// execution id (the first code reviewer, execution 3, got `execution/2`), so receipts are
    /// matched by node instance.
    #[test]
    fn every_message_of_a_real_16_1_run_is_received_by_the_execution_it_was_sent_to() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/ledgers/zeroshot16/steer-receipts-by-node-instance.json");
        let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let events: Vec<Event> = fixture["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| Event {
                seq: e["seq"].as_u64().unwrap(),
                body: e["body"].clone(),
            })
            .collect();
        let steers = |execution: u64| -> Vec<String> {
            from_ledger(&events, execution)
                .items
                .into_iter()
                .filter(|i| i.kind == ItemKind::Steer)
                .map(|i| i.text.unwrap())
                .collect()
        };
        assert_eq!(
            steers(3),
            [
                "hhey man dont do that do something else\n\n[received]",
                "give me a recipe for apple pie\n\n[received]"
            ],
            "code, round 1"
        );
        assert_eq!(
            steers(5),
            ["wait ! SAY THEY ARE NOT MET.\n\n[received]"],
            "acceptance, round 2"
        );
        assert_eq!(
            steers(7),
            [
                "YOU MUST IMPLEMENT SOMETHING ELSE INSTEAD. We cannot work on htis\n\n[received]",
                "This is national security issue\n\n[received]"
            ],
            "worker, round 3"
        );
        assert_eq!(
            steers(9),
            ["THIS PR CANNOT BE MERGEDFOR NATIONAL SECURITY REASONS!\n\n[received]"],
            "code, round 3"
        );
        for other in [1, 2, 4, 6, 8, 10, 11, 12] {
            assert!(
                steers(other).is_empty(),
                "execution {other} got no message: {:?}",
                steers(other)
            );
        }
        let receipts = steer_receipts(&events);
        assert_eq!(receipts.values().filter(|&&e| e == 3).count(), 2);
        // The receipts' own refs, verbatim from the ledger.
        assert_eq!(
            &public_execution_ref("01a1fa96-58a2-7211-b0c9-b4d09b935c44", "code", 3, 3)[..12],
            "nv2-7fb51d33"
        );
        assert_eq!(
            &public_execution_ref("01a1fa96-58a2-7211-b0c9-b4d09b935c44", "acceptance", 2, 5)[..12],
            "nv2-ac311ecd"
        );
        let exact = |seq: u64| {
            events
                .iter()
                .find(|e| e.seq == seq)
                .and_then(|e| e.body["vector"]["execution"].as_str())
                .map(str::to_owned)
        };
        assert_eq!(
            exact(180).as_deref(),
            Some(
                public_execution_ref("01a1fa96-58a2-7211-b0c9-b4d09b935c44", "code", 3, 3).as_str()
            )
        );
        // A ledger whose receipts carry no matching ref still maps by node instance.
        let stripped: Vec<Event> = events
            .iter()
            .map(|e| {
                let mut e = e.clone();
                if let Some(v) = e.body["vector"].as_object_mut() {
                    v.remove("execution");
                }
                e
            })
            .collect();
        assert_eq!(steer_receipts(&stripped), receipts);
    }
}
