//! transcript: ledger lines → transcript for every execution of every fixture run, and the
//! native readers (Claude Code session logs, Copilot session events) directly, including the
//! merge of native and ledger items.

use serde_json::json;

use super::check_serialize;
use super::world::world;
use crate::transcript;

#[test]
fn from_ledger_every_execution() {
    let w = world();
    for s in w.synthetic.iter().chain(&w.recorded) {
        let data = super::projection_goldens::run_data(s);
        let mut ids: Vec<u64> = data
            .events
            .iter()
            .filter_map(|e| e.body["execution"].as_u64())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let all: Vec<_> = ids.iter().map(|id| json!({"execution": id, "transcript": transcript::from_ledger(&data.events, *id)})).collect();
        super::check(
            &format!(
                "transcript/ledger/{}{}",
                super::family_prefix(&s.tags),
                s.name.replace('/', "-")
            ),
            &json!(all),
        );
    }
}

/// Tool calls whose input Zeroshot cut in its log, in both forms (16.1's middle cut and the
/// patched build's `"truncated": true`), with their results, next to an uncut one.
#[test]
fn from_ledger_cut_tool_calls() {
    super::check_serialize(
        "transcript/ledger/cut-tool-calls",
        &transcript::from_ledger(&transcript::cut_tool_lines(), 1),
    );
}

#[test]
fn native_session_logs() {
    let w = world();
    let mut claude: Vec<_> = Vec::new();
    for dir in std::fs::read_dir(w.home.join(".claude/projects"))
        .unwrap()
        .flatten()
    {
        for f in std::fs::read_dir(dir.path()).unwrap().flatten() {
            if f.path().extension().is_some_and(|e| e == "jsonl") {
                claude.push(f.path());
            }
        }
    }
    claude.sort();
    for path in &claude {
        let name = format!(
            "{}-{}",
            path.parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .rsplit("-work-")
                .next()
                .unwrap(),
            path.file_stem().unwrap().to_string_lossy()
        );
        let plain = transcript::from_claude_session(path, &[]).unwrap();
        let steered =
            transcript::from_claude_session(path, &["Use re.sub, not a loop.".to_owned()]).unwrap();
        let usage = transcript::usage_from_claude_session(path).unwrap();
        check_serialize(
            &format!("transcript/claude/{name}"),
            &json!({"transcript": plain, "withSteers": steered.items.iter().filter(|i| i.kind == transcript::ItemKind::Steer).count(), "usage": usage}),
        );
    }
    let copilot = w
        .home
        .join(".copilot/session-state/c0b1107-session-0001/events.jsonl");
    let all = transcript::from_copilot_session(&copilot, &[], None).unwrap();
    let since = transcript::from_copilot_session(
        &copilot,
        &["Use re.sub.".to_owned()],
        Some(1_795_000_000_000 + 60_000 * 9 + 10_000),
    )
    .unwrap();
    check_serialize(
        "transcript/copilot/session",
        &json!({"all": all, "since": since}),
    );
}

#[test]
fn merge_native_with_ledger() {
    let w = world();
    let s = w.scenario("succeeded");
    let data = super::projection_goldens::run_data(s);
    let ledger = transcript::from_ledger(&data.events, 1);
    let path = std::fs::read_dir(w.home.join(".claude/projects"))
        .unwrap()
        .flatten()
        .find(|d| {
            d.file_name()
                .to_string_lossy()
                .ends_with("-work-succeeded-repo")
        })
        .unwrap()
        .path();
    let file = std::fs::read_dir(path)
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    let mut native = transcript::from_claude_session(&file, &[]).unwrap();
    let mut extra = ledger.items.clone();
    extra.push(transcript::Item {
        kind: transcript::ItemKind::Steer,
        at: Some(1_795_000_062_000),
        text: Some("Use re.sub, not a loop.\n\n[queued]".to_owned()),
        detail: None,
        tool: None,
        subagent: false,
    });
    transcript::merge_native_with_ledger(&mut native, extra);
    check_serialize("transcript/merge/succeeded-1", &native);
}
