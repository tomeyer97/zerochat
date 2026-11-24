//! Claude Code's native session logs: `~/.claude/projects/<workspace slug>/<session id>.jsonl`,
//! one record per line (`user`, `assistant`, `system`), one record per content block.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::iso_time::parse_iso_millis;
use super::session::{
    MAX_ERROR_MESSAGE, attach_result, content_text, is_provider_error, single, started_in_window,
    steer_body, strip_auto_tag, tool_call,
};
use super::{Item, ItemKind, ToolResult, Transcript};
use crate::pricing::Usage;

/// Claude Code's project directory name for a working directory.
pub fn claude_project_slug(workspace: &str) -> String {
    workspace
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Finds the session log of one execution: by session id when recorded, otherwise the only
/// session in the workspace's project directory that started inside the execution's window.
/// When several sessions fit the time window (parallel reviewers, Zeroshot 16.1's Active
/// Memory agents in the same workspace, no recorded session id), the one whose first prompt
/// carries the node's own `instructions`.
pub fn find_claude_session_of(
    home: &Path,
    workspace: &str,
    session_id: Option<&str>,
    started_at: Option<u64>,
    ended_at: Option<u64>,
    instructions: Option<&str>,
) -> Option<PathBuf> {
    let dir = home
        .join(".claude/projects")
        .join(claude_project_slug(workspace));
    if let Some(id) = session_id {
        let path = dir.join(format!("{id}.jsonl"));
        return path.is_file().then_some(path);
    }
    let start = started_at?;
    let end = ended_at.unwrap_or(u64::MAX);
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(first) = first_timestamp(&path) else {
            continue;
        };
        if started_in_window(first, start, end) {
            found.push(path);
        }
    }
    if found.len() > 1
        && let Some(needle) = instructions_needle(instructions)
    {
        let matching: Vec<PathBuf> = found
            .iter()
            .filter(|path| first_prompt(path).is_some_and(|p| p.contains(&needle)))
            .cloned()
            .collect();
        if !matching.is_empty() {
            found = matching;
        }
    }
    if found.len() > 1 {
        // Sessions that began only after the node ended (16.1's Active Memory agents start right
        // after it) are not the node's.
        let before_end: Vec<PathBuf> = found
            .iter()
            .filter(|path| first_timestamp(path).is_some_and(|first| first <= end))
            .cloned()
            .collect();
        if !before_end.is_empty() {
            found = before_end;
        }
    }
    single(found)
}

/// What a node's session prompt must contain to be that node's: the first non-empty line of its
/// instructions (up to 120 characters), when it is long enough to tell nodes apart.
fn instructions_needle(instructions: Option<&str>) -> Option<String> {
    let first_line = instructions?
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    Some(first_line.chars().take(120).collect::<String>()).filter(|n| n.len() >= 12)
}

/// The text of a session log's first user message.
fn first_prompt(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().take(50).map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v["type"] != "user" {
            continue;
        }
        return Some(match &v["message"]["content"] {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => continue,
        });
    }
    None
}

fn first_timestamp(path: &Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().take(20).map_while(Result::ok) {
        if let Ok(v) = serde_json::from_str::<Value>(&line)
            && let Some(t) = v["timestamp"].as_str().and_then(parse_iso_millis)
        {
            return Some(t);
        }
    }
    None
}

/// Total usage of a Claude session log. Claude Code writes one record per content block and
/// repeats the message's usage on each, so usage counts once per message id.
pub fn usage_from_claude_session(path: &Path) -> std::io::Result<Usage> {
    let file = std::fs::File::open(path)?;
    let mut by_message: HashMap<String, Usage> = HashMap::new();
    let mut anonymous = Usage::default();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if record["type"] != "assistant" {
            continue;
        }
        let u = &record["message"]["usage"];
        let n = |key: &str| u.get(key).and_then(Value::as_u64).unwrap_or(0);
        let usage = Usage {
            input_tokens: n("input_tokens"),
            output_tokens: n("output_tokens"),
            cache_read_input_tokens: n("cache_read_input_tokens"),
            cache_creation_input_tokens: n("cache_creation_input_tokens"),
        };
        match record["message"]["id"].as_str() {
            Some(id) => {
                by_message.insert(id.to_owned(), usage);
            }
            None => anonymous.add(&usage),
        }
    }
    let mut total = anonymous;
    for usage in by_message.values() {
        total.add(usage);
    }
    Ok(total)
}

/// `steers`: the steer messages the ledger recorded for this execution. Other user messages
/// after the prompt were sent by Zeroshot itself.
pub fn from_claude_session(path: &Path, steers: &[String]) -> std::io::Result<Transcript> {
    let file = std::fs::File::open(path)?;
    let mut reader = ClaudeReader {
        steers,
        items: Vec::new(),
        saw_prompt: false,
    };
    for line in BufReader::new(file).lines() {
        let line = line?;
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        reader.record(&record);
    }
    Ok(Transcript {
        source: "native",
        source_path: Some(path.display().to_string()),
        limitations: Vec::new(),
        items: reader.items,
        copilot_credits: None,
    })
}

/// The chat being read from a Claude session, one record at a time.
struct ClaudeReader<'a> {
    steers: &'a [String],
    items: Vec<Item>,
    /// Whether the node's agent got its prompt yet (the first user message is the prompt).
    saw_prompt: bool,
}

impl ClaudeReader<'_> {
    fn record(&mut self, record: &Value) {
        let at = record["timestamp"].as_str().and_then(parse_iso_millis);
        let subagent = record["isSidechain"].as_bool().unwrap_or(false);
        let content = &record["message"]["content"];
        match record["type"].as_str().unwrap_or("") {
            "user" => self.user(content, at, subagent),
            "assistant" => self.assistant(record, content, at, subagent),
            "system" => {
                if let Some(text) = record["content"].as_str() {
                    self.items
                        .push(Item::message(ItemKind::System, at, text).by_subagent(subagent));
                }
            }
            _ => {}
        }
    }

    /// A user record: the prompt or a later message as plain text, or content blocks (text, and
    /// the results of the model's tool calls).
    fn user(&mut self, content: &Value, at: Option<u64>, subagent: bool) {
        if let Some(text) = content.as_str() {
            self.user_text(text, at, subagent);
            return;
        }
        for block in content.as_array().into_iter().flatten() {
            match block["type"].as_str() {
                Some("tool_result") => {
                    let result = ToolResult {
                        is_error: block["is_error"].as_bool().unwrap_or(false),
                        content: content_text(&block["content"]),
                        at,
                        exit_code: None,
                    };
                    attach_result(
                        &mut self.items,
                        block["tool_use_id"].as_str().unwrap_or(""),
                        result,
                    );
                }
                Some("text") => self.user_text(block["text"].as_str().unwrap_or(""), at, subagent),
                _ => {}
            }
        }
    }

    /// A user message: the prompt (the first one, and every sub-agent's), else a steer, an
    /// interruption notice or Zeroshot's own follow-up (shown without its engine tag).
    fn user_text(&mut self, text: &str, at: Option<u64>, subagent: bool) {
        let kind = if !self.saw_prompt || subagent {
            ItemKind::Prompt
        } else {
            self.later_kind(text)
        };
        self.saw_prompt |= !subagent;
        let text = if kind == ItemKind::Auto {
            strip_auto_tag(text)
        } else {
            text
        };
        self.items
            .push(Item::message(kind, at, text).by_subagent(subagent));
    }

    fn later_kind(&self, text: &str) -> ItemKind {
        if text.starts_with("[Request interrupted") {
            ItemKind::System
        } else if steer_body(text).is_some() || self.steers.iter().any(|s| s.trim() == text.trim())
        {
            ItemKind::Steer
        } else {
            ItemKind::Auto
        }
    }

    /// An assistant record: reasoning, text and tool calls, or Claude Code's own message for an
    /// API error.
    fn assistant(&mut self, record: &Value, content: &Value, at: Option<u64>, subagent: bool) {
        let blocks = content.as_array().into_iter().flatten();
        // Claude Code's synthetic messages for API errors are not something the model said.
        if record["isApiErrorMessage"].as_bool() == Some(true) {
            let text = blocks
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            self.items
                .push(Item::message(ItemKind::Error, at, text).by_subagent(subagent));
            return;
        }
        for block in blocks {
            let item = match block["type"].as_str() {
                Some("thinking") => {
                    let text = block["thinking"].as_str().unwrap_or("");
                    if text.is_empty() {
                        continue;
                    }
                    Item::message(ItemKind::Thinking, at, text)
                }
                Some("text") => {
                    let text = block["text"].as_str().unwrap_or("");
                    let kind = if is_provider_error(text) && text.len() < MAX_ERROR_MESSAGE {
                        ItemKind::Error
                    } else {
                        ItemKind::Text
                    };
                    Item::message(kind, at, text)
                }
                Some("tool_use") => tool_call(at, block),
                _ => continue,
            };
            self.items.push(item.by_subagent(subagent));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slug_matches_claude_code() {
        assert_eq!(
            claude_project_slug("/home/a/my.repo_x"),
            "-home-a-my-repo-x"
        );
    }

    #[test]
    fn parses_claude_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let lines = [
            json!({"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"Do the task"}}),
            json!({"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"content":[{"type":"thinking","thinking":"plan"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}),
            json!({"type":"user","timestamp":"2026-01-01T00:00:02Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt","is_error":false}]}}),
            json!({"type":"user","timestamp":"2026-01-01T00:00:03Z","message":{"content":[{"type":"text","text":"also do X"}]}}),
            json!({"type":"assistant","timestamp":"2026-01-01T00:00:04Z","message":{"content":[{"type":"text","text":"Done"}]}}),
        ];
        std::fs::write(
            &path,
            lines
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let t = from_claude_session(&path, &["also do X".to_owned()]).unwrap();
        let kinds: Vec<_> = t.items.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [
                ItemKind::Prompt,
                ItemKind::Thinking,
                ItemKind::Tool,
                ItemKind::Steer,
                ItemKind::Text
            ]
        );
        assert_eq!(
            t.items[2]
                .tool
                .as_ref()
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .content,
            "a.txt"
        );
        // Without a recorded steer, the same message was Zeroshot's own.
        let t = from_claude_session(&path, &[]).unwrap();
        assert_eq!(t.items[3].kind, ItemKind::Auto);
    }

    #[test]
    fn automatic_turns_drop_the_bracketed_engine_tag() {
        // Zeroshot's own follow-up keeps the instruction, loses the internal tag.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let lines = [
            json!({"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"Do the task"}}),
            json!({"type":"assistant","timestamp":"2026-01-01T00:00:01Z","message":{"content":[{"type":"text","text":"done"}]}}),
            json!({"type":"user","timestamp":"2026-01-01T00:00:02Z","message":{"content":"[structured-output-enforce] You MUST call the StructuredOutput tool to complete this request."}}),
        ];
        std::fs::write(
            &path,
            lines
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let t = from_claude_session(&path, &[]).unwrap();
        assert_eq!(t.items[2].kind, ItemKind::Auto);
        assert_eq!(
            t.items[2].text.as_deref(),
            Some("You MUST call the StructuredOutput tool to complete this request.")
        );
    }

    #[test]
    fn session_usage_counts_each_message_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let usage = json!({"input_tokens": 8, "cache_creation_input_tokens": 700, "cache_read_input_tokens": 30000, "output_tokens": 90});
        let lines = [
            json!({"type":"user","message":{"content":"go"}}),
            json!({"type":"assistant","message":{"id":"m1","content":[{"type":"thinking","thinking":"x"}],"usage":usage}}),
            json!({"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"y"}],"usage":usage}}),
            json!({"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":"z"}],"usage":{"input_tokens":1,"output_tokens":10}}}),
        ];
        std::fs::write(
            &path,
            lines
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let u = usage_from_claude_session(&path).unwrap();
        assert_eq!(
            (
                u.input_tokens,
                u.output_tokens,
                u.cache_read_input_tokens,
                u.cache_creation_input_tokens
            ),
            (9, 100, 30000, 700)
        );
    }

    #[test]
    fn several_sessions_in_the_window_are_told_apart_by_instructions_then_by_start() {
        let home = std::env::temp_dir().join(format!("zerochat-sessions-{}", std::process::id()));
        let dir = home
            .join(".claude/projects")
            .join(claude_project_slug("/w"));
        std::fs::create_dir_all(&dir).unwrap();
        let session = |name: &str, at: &str, prompt: &str| {
            let line = json!({"type": "user", "timestamp": at, "message": {"content": prompt}});
            std::fs::write(dir.join(format!("{name}.jsonl")), format!("{line}\n")).unwrap();
        };
        // 2026-11-20T11:47:50Z = 1795175270000
        session(
            "acceptance",
            "2026-11-20T11:47:50.000Z",
            "Execute this graph node.\nAuthored instructions:\nCheck the change meets the task.",
        );
        session(
            "code",
            "2026-11-20T11:47:50.100Z",
            "Execute this graph node.\nAuthored instructions:\nReview the code for bugs.",
        );
        session(
            "postmortem",
            "2026-11-20T11:48:20.000Z",
            "Authored instructions:\nYou are an Active Memory postmortem specialist.",
        );
        let find = |instructions: Option<&str>, end: u64| {
            find_claude_session_of(
                &home,
                "/w",
                None,
                Some(1795175270000),
                Some(end),
                instructions,
            )
            .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        };
        assert_eq!(
            find(Some("Review the code for bugs."), 1795175300000).as_deref(),
            Some("code")
        );
        assert_eq!(
            find(None, 1795175300000),
            None,
            "two parallel reviewers and no instructions: ambiguous"
        );
        // A session that began after the node ended is another agent's.
        std::fs::remove_file(dir.join("code.jsonl")).unwrap();
        assert_eq!(find(None, 1795175290000).as_deref(), Some("acceptance"));
        let _ = std::fs::remove_dir_all(&home);
    }
}
