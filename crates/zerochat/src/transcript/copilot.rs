//! GitHub Copilot CLI session logs: `<COPILOT_HOME>/session-state/<sessionId>/events.jsonl`, one
//! session event per line (`session.start`, `user.message`, `assistant.message`,
//! `tool.execution_start`/`_complete`, `permission.requested`/`_completed`,
//! `session.compaction_complete`, `session.usage_record`, …).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::iso_time::parse_iso_millis;
use super::session::{
    MAX_ERROR_MESSAGE, attach_result, content_text, is_provider_error, single, started_in_window,
    steer_body, strip_auto_tag,
};
use super::{Item, ItemKind, Permission, Tool, ToolResult, Transcript};

/// Copilot's state directory: `COPILOT_HOME`, else `~/.copilot` (the CLI has no XDG fallback).
pub fn copilot_home(home: &Path) -> PathBuf {
    std::env::var_os("COPILOT_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".copilot"))
}

/// Finds one execution's session log. With a recorded session id: the live file in Copilot's
/// home, else the copy the run retained. Without one (older Zeroshot): the run's only retained
/// log, else the only session in Copilot's home that started in this workspace inside the
/// execution's window.
pub fn find_copilot_session(
    copilot_home: &Path,
    retained: Option<&Path>,
    workspace: Option<&str>,
    session_id: Option<&str>,
    started_at: Option<u64>,
    ended_at: Option<u64>,
) -> Option<PathBuf> {
    let retained_logs = retained.map(retained_logs).unwrap_or_default();
    if let Some(id) = session_id {
        if !is_plain_name(id) {
            return None;
        }
        let live = copilot_home
            .join("session-state")
            .join(id)
            .join("events.jsonl");
        if live.is_file() {
            return Some(live);
        }
        return retained_by_id(&retained_logs, id)
            .or(retained_logs.first())
            .cloned();
    }
    if retained_logs.len() == 1 {
        return retained_logs.into_iter().next();
    }
    let start = started_at?;
    let end = ended_at.unwrap_or(u64::MAX);
    let mut found = Vec::new();
    for entry in std::fs::read_dir(copilot_home.join("session-state"))
        .ok()?
        .flatten()
    {
        let path = entry.path().join("events.jsonl");
        let Some((cwd, first)) = session_start(&path) else {
            continue;
        };
        if workspace.is_some_and(|w| cwd.as_deref() != Some(w)) {
            continue;
        }
        if started_in_window(first, start, end) {
            found.push(path);
        }
    }
    single(found)
}

/// The retained log of session `id`: `…/session-state/<id>/events.jsonl`.
fn retained_by_id<'a>(retained_logs: &'a [PathBuf], id: &str) -> Option<&'a PathBuf> {
    retained_logs.iter().find(|path| {
        path.parent()
            .and_then(|dir| dir.file_name())
            .is_some_and(|name| name == id)
    })
}

fn is_plain_name(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `events.jsonl` files under a retained execution directory (any depth, a few levels).
fn retained_logs(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth < 8 {
                walk(&path, depth + 1, out);
            } else if path.file_name().is_some_and(|n| n == "events.jsonl")
                && path.components().any(|c| c.as_os_str() == "session-state")
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out.sort();
    out
}

/// `session.start`'s working directory and start time.
fn session_start(path: &Path) -> Option<(Option<String>, u64)> {
    let file = std::fs::File::open(path).ok()?;
    let mut line = Vec::new();
    BufReader::new(std::io::Read::take(file, 1 << 20))
        .read_until(b'\n', &mut line)
        .ok()?;
    let v: Value = serde_json::from_slice(&line).ok()?;
    if v["type"] != "session.start" {
        return None;
    }
    let data = &v["data"];
    let at = data["startTime"]
        .as_str()
        .or(v["timestamp"].as_str())
        .and_then(parse_iso_millis)?;
    Some((data["context"]["cwd"].as_str().map(str::to_owned), at))
}

/// Builds the chat from a Copilot session log. `steers`: the messages the ledger recorded as
/// steers for this execution; other user messages after the prompt are Zeroshot's own.
/// A line that does not parse (the last one while Copilot is writing it) is skipped. A resumed
/// session's log also holds earlier executions: records before `since` are left out.
pub fn from_copilot_session(
    path: &Path,
    steers: &[String],
    since: Option<u64>,
) -> std::io::Result<Transcript> {
    let file = std::fs::File::open(path)?;
    let mut reader = CopilotReader {
        steers,
        items: Vec::new(),
        saw_prompt: false,
        nano_aiu: None,
        permissions: Vec::new(),
    };
    for line in BufReader::new(file).split(b'\n') {
        let line = line?;
        let Ok(record) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let at = record["timestamp"].as_str().and_then(parse_iso_millis);
        if since.is_some_and(|since| at.is_some_and(|at| at < since)) {
            continue;
        }
        reader.record(&record, at);
    }
    Ok(Transcript {
        source: "native",
        source_path: Some(path.display().to_string()),
        limitations: Vec::new(),
        items: reader.items,
        copilot_credits: reader.nano_aiu.map(|nano_aiu| nano_aiu / 1e9),
    })
}

/// The chat being read from a Copilot session, one event at a time.
struct CopilotReader<'a> {
    steers: &'a [String],
    items: Vec<Item>,
    /// Whether the prompt (the first user message) was read.
    saw_prompt: bool,
    /// Copilot's billing figure so far (`totalNanoAiu` summed); `None` until an event has one.
    nano_aiu: Option<f64>,
    /// Permission request id → tool call id, until the answer arrives.
    permissions: Vec<(String, String)>,
}

impl CopilotReader<'_> {
    fn record(&mut self, record: &Value, at: Option<u64>) {
        let data = &record["data"];
        match record["type"].as_str().unwrap_or("") {
            "user.message" => self.user_message(data, at),
            "assistant.message" => self.assistant_message(data, at),
            "tool.execution_start" => self.push_tool(
                data["toolCallId"].as_str(),
                data["toolName"].as_str(),
                &data["arguments"],
                at,
                is_subagent(data),
            ),
            "tool.execution_complete" => self.tool_complete(data, at),
            "permission.requested" => self.permission_requested(data, at),
            "permission.completed" => self.permission_completed(data, at),
            "session.compaction_complete" => self.compaction(data, at),
            "session.usage_record" => {
                self.add_nano_aiu(&data["usage"]["copilotUsage"]["totalNanoAiu"])
            }
            "session.error" => {
                if let Some(message) = data["message"].as_str() {
                    self.items.push(Item::message(ItemKind::Error, at, message));
                }
            }
            "system.notification" => {
                if let Some(text) = notification_text(data) {
                    self.items.push(Item::message(ItemKind::System, at, text));
                }
            }
            _ => {}
        }
    }

    /// The prompt, then steers (delivered as steering, or recorded by the ledger) and Zeroshot's
    /// own follow-ups (shown without their engine tag).
    fn user_message(&mut self, data: &Value, at: Option<u64>) {
        let text = data["content"].as_str().unwrap_or("");
        if !self.saw_prompt {
            self.saw_prompt = true;
            self.items.push(Item::message(ItemKind::Prompt, at, text));
            return;
        }
        let body = steer_body(text).unwrap_or(text);
        if data["delivery"] == "steering" || self.steers.iter().any(|s| s.trim() == body.trim()) {
            self.items.push(Item::message(ItemKind::Steer, at, body));
        } else {
            self.items
                .push(Item::message(ItemKind::Auto, at, strip_auto_tag(text)));
        }
    }

    /// Reasoning, text, and the tool calls the model asks for.
    fn assistant_message(&mut self, data: &Value, at: Option<u64>) {
        let subagent = is_subagent(data);
        if let Some(reasoning) = non_blank(&data["reasoningText"]) {
            self.items
                .push(Item::message(ItemKind::Thinking, at, reasoning).by_subagent(subagent));
        }
        if let Some(text) = non_blank(&data["content"]) {
            let kind = if is_provider_error(text) && text.len() < MAX_ERROR_MESSAGE {
                ItemKind::Error
            } else {
                ItemKind::Text
            };
            self.items
                .push(Item::message(kind, at, text).by_subagent(subagent));
        }
        for request in data["toolRequests"].as_array().into_iter().flatten() {
            self.push_tool(
                request["toolCallId"].as_str(),
                request["name"].as_str(),
                &request["arguments"],
                at,
                subagent,
            );
        }
    }

    /// A tool call, once: Copilot announces it in the assistant message and again when it starts.
    fn push_tool(
        &mut self,
        id: Option<&str>,
        name: Option<&str>,
        input: &Value,
        at: Option<u64>,
        subagent: bool,
    ) {
        let id = id.unwrap_or("");
        if !id.is_empty() && tool_mut(&mut self.items, id).is_some() {
            return;
        }
        self.items
            .push(Item::tool(at, id, name.unwrap_or("tool"), input.clone()).by_subagent(subagent));
    }

    /// The result, or for a failed call its error message.
    fn tool_complete(&mut self, data: &Value, at: Option<u64>) {
        let failed = data["success"].as_bool() == Some(false);
        let error_message = data["error"]["message"].as_str().filter(|_| failed);
        let content = error_message
            .map(str::to_owned)
            .unwrap_or_else(|| content_text(&data["result"]["content"]));
        let result = ToolResult {
            is_error: failed,
            content,
            at,
            exit_code: data["shellExecution"]["exitCode"].as_i64(),
        };
        attach_result(
            &mut self.items,
            data["toolCallId"].as_str().unwrap_or(""),
            result,
        );
    }

    /// A permission request goes on its tool call (a notice when there is no call to put it on).
    fn permission_requested(&mut self, data: &Value, at: Option<u64>) {
        let request = &data["permissionRequest"];
        let kind = request["kind"].as_str().unwrap_or("permission").to_owned();
        let call = request["toolCallId"]
            .as_str()
            .or(data["promptRequest"]["toolCallId"].as_str())
            .unwrap_or("");
        match tool_mut(&mut self.items, call) {
            Some(tool) => tool.permission = Some(Permission { kind, result: None }),
            None => self.items.push(Item::message(
                ItemKind::System,
                at,
                format!("Permission requested: {kind}"),
            )),
        }
        if let Some(id) = data["requestId"].as_str() {
            self.permissions.push((id.to_owned(), call.to_owned()));
        }
    }

    /// The answer goes on the requested permission; an answer other than approval with no
    /// request to go on is a notice of its own.
    fn permission_completed(&mut self, data: &Value, at: Option<u64>) {
        let result = data["result"]["kind"]
            .as_str()
            .unwrap_or("answered")
            .to_owned();
        let request = data["requestId"].as_str().unwrap_or("");
        let requested_for = || {
            self.permissions
                .iter()
                .find(|(r, _)| r == request)
                .map(|(_, call)| call.clone())
        };
        let call = data["toolCallId"]
            .as_str()
            .map(str::to_owned)
            .or_else(requested_for)
            .unwrap_or_default();
        self.permissions.retain(|(r, _)| r != request);
        if let Some(permission) =
            tool_mut(&mut self.items, &call).and_then(|t| t.permission.as_mut())
        {
            permission.result = Some(result);
        } else if !result.starts_with("approved") {
            self.items.push(Item::message(
                ItemKind::System,
                at,
                format!("Permission {result}"),
            ));
        }
    }

    /// `Context compacted: 142.6k → 57.2k tokens`, with the summary behind it.
    fn compaction(&mut self, data: &Value, at: Option<u64>) {
        let tokens = (
            data["preCompactionTokens"].as_u64(),
            data["postCompactionTokens"].as_u64(),
        );
        let text = match (data["success"].as_bool(), tokens) {
            (Some(false), _) => "Context compaction failed".to_owned(),
            (_, (Some(pre), Some(post))) => format!(
                "Context compacted: {} → {} tokens",
                short_count(pre),
                short_count(post)
            ),
            _ => "Context compacted".to_owned(),
        };
        let summary = non_blank(&data["summaryContent"]).map(str::to_owned);
        self.items
            .push(Item::message(ItemKind::Compaction, at, text).with_detail(summary));
        self.add_nano_aiu(&data["compactionTokensUsed"]["copilotUsage"]["totalNanoAiu"]);
    }

    fn add_nano_aiu(&mut self, nano_aiu: &Value) {
        if let Some(n) = nano_aiu.as_f64() {
            *self.nano_aiu.get_or_insert(0.0) += n;
        }
    }
}

/// An event of a sub-agent (one started by a tool call of the node's agent).
fn is_subagent(data: &Value) -> bool {
    data["parentToolCallId"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
}

/// A string with something besides whitespace in it.
fn non_blank(value: &Value) -> Option<&str> {
    value.as_str().filter(|t| !t.trim().is_empty())
}

fn tool_mut<'a>(items: &'a mut [Item], id: &str) -> Option<&'a mut Tool> {
    if id.is_empty() {
        return None;
    }
    items
        .iter_mut()
        .rev()
        .filter_map(|i| i.tool.as_mut())
        .find(|t| t.id == id)
}

/// `142641` → `142.6k`.
fn short_count(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// A background-shell notice in a few words, else the notification text without its tags.
fn notification_text(data: &Value) -> Option<String> {
    let kind = &data["kind"];
    if kind["type"] == "shell_completed" {
        let what = kind["description"]
            .as_str()
            .or(kind["shellId"].as_str())
            .unwrap_or("shell command");
        return Some(match kind["exitCode"].as_i64() {
            Some(0) => format!("Background command finished: {what}"),
            Some(code) => format!("Background command failed (exit {code}): {what}"),
            None => format!("Background command finished: {what}"),
        });
    }
    let text = data["content"].as_str()?;
    let text = text
        .replace("<system_notification>", "")
        .replace("</system_notification>", "");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(dir: &Path, lines: &[Value], tail: &str) -> PathBuf {
        let path = dir.join("events.jsonl");
        let mut text = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        text.push('\n');
        text.push_str(tail);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn ev(kind: &str, at: &str, data: Value) -> Value {
        json!({"type": kind, "timestamp": format!("2026-11-05T10:00:{at}Z"), "data": data})
    }

    fn session() -> Vec<Value> {
        vec![
            ev(
                "session.start",
                "00.000",
                json!({"sessionId": "s1", "startTime": "2026-11-05T10:00:00.000Z", "context": {"cwd": "/work/repo"}}),
            ),
            ev(
                "user.message",
                "01.000",
                json!({"content": "Execute this graph node", "delivery": "idle"}),
            ),
            ev(
                "system.message",
                "01.100",
                json!({"role": "system", "content": "You are GitHub Copilot"}),
            ),
            // Reasoning-only message that asks for a tool.
            ev(
                "assistant.message",
                "02.000",
                json!({
                    "content": "",
                    "reasoningText": "Look around first.",
                    "reasoningOpaque": "x",
                    "toolRequests": [{"toolCallId": "t1", "name": "bash", "arguments": {"command": "git status", "description": "Inspect"}}],
                }),
            ),
            ev(
                "session.usage_record",
                "02.000",
                json!({"usage": {"inputTokens": 100, "cost": 15.0, "copilotUsage": {"totalNanoAiu": 2_500_000_000u64}}}),
            ),
            ev(
                "tool.execution_start",
                "02.100",
                json!({"toolCallId": "t1", "toolName": "bash", "arguments": {"command": "git status", "description": "Inspect"}}),
            ),
            ev(
                "permission.requested",
                "02.200",
                json!({"requestId": "p1", "permissionRequest": {"kind": "shell", "toolCallId": "t1"}}),
            ),
            ev(
                "permission.completed",
                "02.300",
                json!({"requestId": "p1", "toolCallId": "t1", "result": {"kind": "approved"}}),
            ),
            ev(
                "tool.execution_complete",
                "03.000",
                json!({"toolCallId": "t1", "success": true, "result": {"content": "fatal: no repo"}, "shellExecution": {"exitCode": 128}}),
            ),
            ev(
                "assistant.message",
                "04.000",
                json!({"content": "", "toolRequests": [{"toolCallId": "t2", "name": "view", "arguments": {"path": "/work/repo/missing.md"}}]}),
            ),
            ev(
                "tool.execution_start",
                "04.100",
                json!({"toolCallId": "t2", "toolName": "view", "arguments": {"path": "/work/repo/missing.md"}}),
            ),
            ev(
                "tool.execution_complete",
                "04.200",
                json!({"toolCallId": "t2", "success": false, "error": {"message": "Path does not exist"}}),
            ),
            ev(
                "session.compaction_start",
                "05.000",
                json!({"currentTokens": 160000}),
            ),
            ev(
                "session.compaction_complete",
                "06.000",
                json!({
                    "success": true,
                    "preCompactionTokens": 142641,
                    "postCompactionTokens": 57200,
                    "summaryContent": "<overview>Did things</overview>",
                    "compactionTokensUsed": {"copilotUsage": {"totalNanoAiu": 500_000_000u64}},
                }),
            ),
            ev(
                "user.message",
                "07.000",
                json!({
                    "content": "Message from the user, sent while you were working. They read your messages live: answer in a message:\nstop at 10",
                    "delivery": "steering",
                }),
            ),
            ev(
                "user.message",
                "08.000",
                json!({"content": "[structured-output-enforce] Answer with the schema.", "delivery": "idle"}),
            ),
            ev(
                "system.notification",
                "08.500",
                json!({
                    "content": "<system_notification>x</system_notification>",
                    "kind": {"type": "shell_completed", "shellId": "yarn", "exitCode": 0, "description": "Install deps"},
                }),
            ),
            ev(
                "assistant.message",
                "09.000",
                json!({"content": "{\"response\":{\"ok\":true}}"}),
            ),
        ]
    }

    /// The first tool item called `name`.
    fn tool<'a>(t: &'a Transcript, name: &str) -> &'a Tool {
        t.items
            .iter()
            .filter_map(|i| i.tool.as_ref())
            .find(|tool| tool.name == name)
            .unwrap()
    }

    #[test]
    fn parses_a_copilot_session_into_chat_items() {
        let dir = tempfile::tempdir().unwrap();
        // The last line is cut mid-record, as while Copilot is still writing it.
        let path = write(
            dir.path(),
            &session(),
            "{\"type\":\"assistant.message\",\"data\":{\"content\":\"hal",
        );
        let t = from_copilot_session(&path, &[], None).unwrap();
        let chat: Vec<(ItemKind, Option<&str>)> = t
            .items
            .iter()
            .map(|i| (i.kind, i.text.as_deref()))
            .collect();
        assert_eq!(
            chat,
            [
                (ItemKind::Prompt, Some("Execute this graph node")),
                (ItemKind::Thinking, Some("Look around first.")),
                (ItemKind::Tool, None),
                (ItemKind::Tool, None),
                (
                    ItemKind::Compaction,
                    Some("Context compacted: 142.6k → 57.2k tokens")
                ),
                (ItemKind::Steer, Some("stop at 10")),
                (ItemKind::Auto, Some("Answer with the schema.")),
                (
                    ItemKind::System,
                    Some("Background command finished: Install deps")
                ),
                (ItemKind::Text, Some("{\"response\":{\"ok\":true}}")),
            ]
        );
        let bash = tool(&t, "bash");
        assert_eq!(
            (bash.name.as_str(), bash.input["command"].as_str()),
            ("bash", Some("git status"))
        );
        let result = bash.result.as_ref().unwrap();
        assert_eq!(
            (result.is_error, result.exit_code, result.content.as_str()),
            (false, Some(128), "fatal: no repo")
        );
        let permission = bash.permission.as_ref().unwrap();
        assert_eq!(
            (permission.kind.as_str(), permission.result.as_deref()),
            ("shell", Some("approved"))
        );
        let view = tool(&t, "view");
        assert!(view.result.as_ref().unwrap().is_error);
        assert_eq!(view.result.as_ref().unwrap().content, "Path does not exist");
        let compaction = t
            .items
            .iter()
            .find(|i| i.kind == ItemKind::Compaction)
            .unwrap();
        assert_eq!(
            compaction.detail.as_deref(),
            Some("<overview>Did things</overview>")
        );
        // Copilot's own figure: nano-AIU summed over calls and the compaction, in AI credits.
        assert_eq!(t.copilot_credits, Some(3.0));
    }

    #[test]
    fn reasoning_only_messages_and_tools_without_a_start_still_show() {
        let dir = tempfile::tempdir().unwrap();
        let lines = vec![
            ev("user.message", "01.000", json!({"content": "go"})),
            ev(
                "assistant.message",
                "02.000",
                json!({"content": "", "reasoningText": "Thinking only."}),
            ),
            ev(
                "assistant.message",
                "03.000",
                json!({"content": "", "toolRequests": [{"toolCallId": "t9", "name": "edit", "arguments": {"path": "a.rs", "old_str": "a", "new_str": "b"}}]}),
            ),
        ];
        let path = write(dir.path(), &lines, "");
        let t = from_copilot_session(&path, &[], None).unwrap();
        let kinds: Vec<_> = t.items.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [ItemKind::Prompt, ItemKind::Thinking, ItemKind::Tool]
        );
        assert!(
            t.items[2].tool.as_ref().unwrap().result.is_none(),
            "the run stopped before the tool finished"
        );
        assert_eq!(t.copilot_credits, None);
        // A resumed session: only this execution's part.
        let t =
            from_copilot_session(&path, &[], parse_iso_millis("2026-11-05T10:00:02.500Z")).unwrap();
        assert_eq!(
            t.items.iter().map(|i| i.kind).collect::<Vec<_>>(),
            [ItemKind::Tool]
        );
    }

    #[test]
    fn finds_the_session_by_id_retained_copy_or_workspace_and_time() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join("session-state");
        let start = parse_iso_millis("2026-11-05T10:00:00.000Z").unwrap();
        for (id, cwd, at) in [
            ("a", "/work/repo", "2026-11-05T10:00:01.000Z"),
            ("b", "/work/other", "2026-11-05T10:00:01.000Z"),
            ("c", "/work/repo", "2026-11-05T12:00:00.000Z"),
        ] {
            std::fs::create_dir_all(state.join(id)).unwrap();
            let start = json!({"type": "session.start", "timestamp": at, "data": {"sessionId": id, "startTime": at, "context": {"cwd": cwd}}});
            std::fs::write(state.join(id).join("events.jsonl"), format!("{start}\n")).unwrap();
        }
        let found = |id: Option<&str>, retained: Option<&Path>| {
            find_copilot_session(
                home.path(),
                retained,
                Some("/work/repo"),
                id,
                Some(start),
                Some(start + 60_000),
            )
        };
        assert_eq!(found(Some("c"), None), Some(state.join("c/events.jsonl")));
        assert_eq!(
            found(None, None),
            Some(state.join("a/events.jsonl")),
            "same workspace, started with the execution"
        );
        assert_eq!(found(Some("../a"), None), None);
        // A retained copy wins when the live file is gone.
        let run = tempfile::tempdir().unwrap();
        let kept = run.path().join("session-state/gone/events.jsonl");
        std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
        std::fs::write(&kept, "{}\n").unwrap();
        assert_eq!(found(Some("gone"), Some(run.path())), Some(kept.clone()));
        assert_eq!(found(None, Some(run.path())), Some(kept));
    }
}
