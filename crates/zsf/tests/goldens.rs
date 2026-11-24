//! Golden tests for zsf (tests/GOLDENS.md): the scheduler driven through every slice transition
//! by the scripted fake Zeroshot, with state.json, the event log and `zsf status` (report and
//! text) pinned after each step; and the model's tables (statuses, attempt kinds, slugs, time
//! formats, unknown event kinds). Paths, commit ids and timestamps are normalised.

mod common;

#[path = "../../zerochat/tests/goldens/golden.rs"]
#[allow(dead_code)]
mod golden;

use common::*;
use serde_json::{Value, json};
use zsf::model::{Action, AttemptKind, SliceStatus, State};

fn dir() -> std::path::PathBuf {
    golden::golden_dir(env!("CARGO_MANIFEST_DIR"))
}

/// Times → 0, 40-hex commit ids → `<commit>`, the temp root → `<root>`.
fn normalize(value: &mut Value, root: &str) {
    match value {
        Value::String(s) => {
            *s = s.replace(root, "<root>");
            let mut out = String::new();
            let chars: Vec<char> = s.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let run = chars[i..]
                    .iter()
                    .take_while(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                    .count();
                if run >= 40 && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
                    out.push_str("<commit>");
                    i += run;
                } else if run == 10
                    && (i == 0 || chars[i - 1] == ' ')
                    && chars[i..i + 10].iter().any(|c| c.is_ascii_digit())
                    && chars.get(i + 10).is_none_or(|c| !c.is_ascii_alphanumeric())
                {
                    out.push_str("<short>");
                    i += run;
                } else {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            *s = out;
        }
        Value::Number(_) => {}
        Value::Array(items) => items.iter_mut().for_each(|v| normalize(v, root)),
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "launch_key" {
                    if let Some(key) = v
                        .as_str()
                        .and_then(|s| s.rsplit_once('-'))
                        .map(|(head, _)| format!("{head}-<time>"))
                    {
                        *v = json!(key);
                    }
                } else if k.ends_with("_ms") || k == "ts" {
                    if v.is_number() {
                        *v = json!(0);
                    }
                } else {
                    normalize(v, root);
                }
            }
        }
        _ => {}
    }
}

fn check(name: &str, mut value: Value, root: &std::path::Path) {
    normalize(&mut value, &root.display().to_string());
    golden::check_json_in(&dir(), name, &value);
}

fn snapshot(env: &Env, e: &zsf::engine::Engine) -> Value {
    let plan = env.plan();
    let report = zsf::status::build(&env.feature, &plan, &e.state, None);
    let text = zsf::status::render(&report);
    // Durations of running attempts depend on the clock: keep the line shape only.
    let text: Vec<String> = text
        .lines()
        .map(|l| {
            if l.contains(" attempt ") {
                l.rsplit_once(", ")
                    .map(|(h, _)| format!("{h}, <duration>"))
                    .unwrap_or(l.to_owned())
            } else {
                l.to_owned()
            }
        })
        .collect();
    let events: Vec<Value> = env
        .events()
        .iter()
        .map(|ev| serde_json::to_value(ev).unwrap())
        .collect();
    json!({"state": serde_json::to_value(&e.state).unwrap(), "report": serde_json::to_value(&report).unwrap(), "status": text, "events": events})
}

#[test]
fn scheduler_walk() {
    // 01 ok; 02 quality failure then retry with notes; 03 checkpoint; 04 after 03; 05 launch fails;
    // 06 skipped while running; 07 infra failure then infra retry ok.
    let env = setup(
        &[
            ("01", &[], false),
            ("02", &[], false),
            ("03", &[], true),
            ("04", &["03"], false),
            ("05", &[], false),
            ("06", &[], false),
            ("07", &[], false),
        ],
        |c| c.limits.max_parallel = 1,
    );
    env.fake.script("01", vec![succeeds(&[("01.txt", "one")])]);
    env.fake.script(
        "02",
        vec![
            fails("change_attempts_exhausted"),
            succeeds(&[("02.txt", "two")]),
        ],
    );
    env.fake
        .script("03", vec![succeeds(&[("03.txt", "three")])]);
    env.fake.script("04", vec![succeeds(&[("04.txt", "four")])]);
    env.fake
        .script("05", vec![Run::LaunchFails("zeroshot run: exit 2")]);
    env.fake.script("06", vec![runs(&[("06.txt", "six")])]);
    env.fake.script(
        "07",
        vec![fails("runtime_lost"), succeeds(&[("07.txt", "seven")])],
    );
    let root = env.repo.parent().unwrap().to_path_buf();
    let mut e = env.engine();
    check("zsf/walk/00-initial", snapshot(&env, &e), &root);
    e = env.started();
    e.run_until_idle().unwrap();
    check("zsf/walk/01-started", snapshot(&env, &e), &root);
    e.run_until_idle().unwrap();
    check(
        "zsf/walk/02-merged-and-checkpoint",
        snapshot(&env, &e),
        &root,
    );
    e.apply(&Action::Approve { slice: "03".into() }).unwrap();
    e.apply(&Action::Retry {
        slice: "02".into(),
        notes: Some("Add the missing test.".into()),
        fresh: false,
    })
    .unwrap();
    e.apply(&Action::Skip { slice: "06".into() }).unwrap();
    e.run_until_idle().unwrap();
    check(
        "zsf/walk/03-approved-retried-skipped",
        snapshot(&env, &e),
        &root,
    );
    for _ in 0..4 {
        e.run_until_idle().unwrap();
    }
    check("zsf/walk/04-settled", snapshot(&env, &e), &root);
    let refused: Vec<Value> = [
        Action::Approve { slice: "01".into() },
        Action::Retry { slice: "01".into(), notes: None, fresh: false },
        Action::Skip { slice: "99".into() },
        Action::Accept { slice: "05".into() },
        Action::Unpause,
        Action::SetMaxParallel { max_parallel: 0 },
    ]
    .iter()
    .map(|a| json!({"action": serde_json::to_value(a).unwrap(), "result": e.apply(a).map_err(|err| format!("{err:#}"))}))
    .collect();
    check("zsf/walk/05-refusals", json!(refused), &root);
}

#[test]
fn model_tables() {
    let statuses: Vec<Value> = [SliceStatus::Pending, SliceStatus::Launching, SliceStatus::Running, SliceStatus::Integrating, SliceStatus::WaitingForReview, SliceStatus::Merged, SliceStatus::Failed, SliceStatus::Interrupted, SliceStatus::Skipped]
        .iter()
        .map(|s| {
            let st = zsf::model::SliceState { status: *s, ..Default::default() };
            let approved = zsf::model::SliceState { status: *s, approved: true, ..Default::default() };
            json!({"status": s.as_str(), "serde": serde_json::to_value(s).unwrap(), "display": st.display_status(false), "displayCheckpoint": st.display_status(true), "displayApproved": approved.display_status(true)})
        })
        .collect();
    let kinds: Vec<Value> = [
        AttemptKind::First,
        AttemptKind::InfraRetry,
        AttemptKind::Rerun,
        AttemptKind::ManualRetry,
        AttemptKind::GateRerun,
    ]
    .iter()
    .map(|k| json!([k.as_str(), serde_json::to_value(k).unwrap()]))
    .collect();
    let slugs: Vec<Value> = [
        "Add sub()",
        "  my Title: here ",
        "Ünïcode — dashes",
        "a very long title that goes on and on and on past forty characters",
        "---",
        "",
    ]
    .iter()
    .map(|t| json!([t, zsf::model::slug(t)]))
    .collect();
    let durations: Vec<Value> = [
        0u64, 59_000, 60_000, 3_599_000, 3_600_000, 3_725_000, 86_400_000, 90_061_000,
    ]
    .iter()
    .map(|ms| {
        json!([
            ms,
            zsf::time::duration(*ms),
            zsf::time::utc(*ms + 1_795_000_000_000)
        ])
    })
    .collect();
    let unknown: zsf::model::Event =
        serde_json::from_str(r#"{"ts":1,"kind":"from_the_future","detail":"x"}"#).unwrap();
    let state: State = serde_json::from_str(r#"{"version":1,"active":true,"slices":{"01":{"status":"failed","attempts":[{"n":1,"kind":"infra-retry","task_file":"t.md","started_ms":5,"result":"integration_failed"}],"retry":{"fresh":true},"job":{"job":"integrate","accept":true}}}}"#).unwrap();
    golden::check_json_in(
        &dir(),
        "zsf/model-tables",
        &json!({"statuses": statuses, "attemptKinds": kinds, "slugs": slugs, "time": durations, "unknownEvent": serde_json::to_value(&unknown).unwrap(), "stateRoundTrip": serde_json::to_value(&state).unwrap(), "oneLine": zsf::status::one_line("\n-----\nRan 4 tests\n\nOK\n")}),
    );
}
