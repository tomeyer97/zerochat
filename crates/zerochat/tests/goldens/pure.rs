//! Pure transforms as tables: input → output for every branch (diagnosis kinds, HTTP-status
//! parsing, reset times, durations, verdicts, retry/rate-limit wording, pricing, graph walks,
//! config parsing, run context, small helpers).

use serde_json::{Value, json};

use super::check;
use super::world::world;
use crate::{config, context, harness, pricing, projection, source, transcript};

fn diagnosis_json(d: Option<projection::Diagnosis>) -> Value {
    d.map(|d| json!({"kind": d.kind, "model": d.model, "http": d.http, "resets": d.resets}))
        .unwrap_or(Value::Null)
}

pub const ERROR_LINES: &[&str] = &[
    "API Error: 403 {\"type\":\"error\",\"error\":{\"type\":\"permission_error\"}} There's an issue with the selected model (claude-opus-4-1). ",
    "does not have permission to use this model",
    "API Error: 401 Unauthorized",
    "Failed to authenticate. API Error: 401",
    "Invalid API key · Please run /login",
    "Claude AI usage limit reached|1795000000",
    "You've hit your limit · resets 9:30am (UTC)",
    "Weekly limit reached · resets 11am (UTC).",
    "Credit balance is too low",
    "API Error: 429 rate_limit_error",
    "status 429 Too Many Requests",
    "not_found_error: model: claude-x",
    "{\"model\":\"gpt-9\",\"error\":\"model_not_found\"}",
    "API Error: 529 overloaded_error",
    "HTTP 502 Bad Gateway",
    "503 Service Unavailable",
    "\"status\":500 Internal Server Error",
    "StreamNoEventsError",
    "socket hang up",
    "Request timed out.",
    "malformed API response",
    "prompt is too long",
    "ENOSPC: no space left on device",
    "connect ECONNREFUSED 127.0.0.1:443",
    "getaddrinfo ENOTFOUND api.anthropic.com",
    "spawn codex ENOENT: executable file not found",
    "crash after 401.3s (pid 503)",
    "line 404 of the file",
    "everything is fine",
];

#[test]
fn diagnose_and_http_statuses() {
    let rows: Vec<Value> = ERROR_LINES
        .iter()
        .map(|line| {
            let lines: Vec<String> = line.split('|').map(str::to_owned).collect();
            json!({"lines": lines, "diagnosis": diagnosis_json(projection::diagnose(&lines)), "httpStatuses": projection::http_statuses(line)})
        })
        .collect();
    check("pure/diagnose", &json!(rows));
}

#[test]
fn reset_times_and_durations() {
    let after = 1_795_000_000_000u64; // 08:26:40 UTC
    let resets: Vec<Value> = [
        "9:30am (UTC)",
        "11am (UTC)",
        "12am (UTC)",
        "12pm (UTC)",
        "21:05 (UTC)",
        "8:00am (UTC)",
        "9:30am (CET)",
        "25:00 (UTC)",
        "noon",
    ]
    .iter()
    .map(|t| json!({"text": t, "after": after, "resetsAt": projection::parse_reset(t, after)}))
    .collect();
    let durations: Vec<Value> = [
        0u64, 999, 1_000, 45_400, 59_999, 60_000, 250_000, 1_800_000, 3_599_000, 3_600_000,
        3_660_000, 7_325_000, 90_000_000,
    ]
    .iter()
    .map(|ms| json!([ms, projection::human_ms(*ms)]))
    .collect();
    check("pure/reset-times", &json!(resets));
    check("pure/human-ms", &json!(durations));
}

#[test]
fn verdicts_retries_rate_limits() {
    let signals = [
        json!({"verdict": "accepted"}),
        json!({"verdict": "rejected"}),
        json!({"a": "approved", "b": "changes_requested"}),
        json!({"a": "pass", "b": "ok"}),
        json!({"verdict": "maybe"}),
        json!({}),
        json!("not an object"),
        json!({"verdict": "CI_FAILED"}),
        json!({"x": 1}),
    ];
    let verdicts: Vec<Value> = signals
        .iter()
        .map(|s| json!({"signals": s, "accepted": projection::verdict_accepted(s)}))
        .collect();
    let retries: Vec<Value> = [
        "Claude API retry 2/10: rate_limit",
        "Claude API retry 1/10: ",
        "Claude API retry 3/10: unknown",
        "Claude API retry 4/10: overloaded_error",
        "Claude API retry x",
        "something else",
    ]
    .iter()
    .map(|t| json!([t, projection::humanize_retry(t)]))
    .collect();
    let limits: Vec<Value> = [
        "Claude rate limit: status=allowed, type=five_hour, resetsAt=2026-11-18T13:00:00Z",
        "Claude rate limit: status=allowed_warning, type=seven_day, utilization=33%, resetsAt=2026-11-20T00:00:00Z",
        "Claude rate limit: status=allowed_warning, type=seven_day, utilization=79.95%, resetsAt=2026-11-20T00:00:00Z",
        "Claude rate limit: status=allowed_warning, type=seven_day_opus, utilization=80%",
        "Claude rate limit: status=allowed_warning, type=five_hour",
        "Claude rate limit: status=rejected, type=overage_daily, resetsAt=garbage",
        "Not a rate limit line",
    ]
    .iter()
    .map(|t| json!([t, projection::parse_rate_limit_line(t).map(|p| json!({"window": p.window, "level": p.level, "text": p.text, "resetsAt": p.resets_at}))]))
    .collect();
    let states: Vec<Value> = [
        projection::ExecState::Running,
        projection::ExecState::Succeeded,
        projection::ExecState::Accepted,
        projection::ExecState::Rejected,
        projection::ExecState::Failed,
        projection::ExecState::Voided,
        projection::ExecState::Interrupted,
    ]
    .iter()
    .map(|s| json!([serde_json::to_value(s).unwrap(), projection::state_name(*s)]))
    .collect();
    check("pure/verdicts", &json!(verdicts));
    check("pure/retry-wording", &json!(retries));
    check("pure/rate-limit-lines", &json!(limits));
    check("pure/exec-state-names", &json!(states));
}

#[test]
fn pricing_estimates() {
    let usage = pricing::Usage::from_json(
        &json!({"inputTokens": 100_000, "outputTokens": 20_000, "cacheReadInputTokens": 1_000_000, "cacheCreationInputTokens": 50_000}),
    );
    let codex = pricing::Usage::from_harness_json(
        &json!({"inputTokens": 100_000, "outputTokens": 20_000, "cacheReadInputTokens": 60_000}),
        Some("codex"),
    );
    let models = [
        "claude-haiku-4-5",
        "claude-haiku-4-5-20251001",
        "claude-sonnet-4-5",
        "claude-sonnet-5",
        "claude-opus-4-1",
        "claude-opus-5-5",
        "claude-3-5-haiku",
        "gpt-5",
        "gpt-5-codex",
        "gpt-5-mini",
        "o3",
        "gemini-2.5-pro",
        "unknown-model",
    ];
    let mut rows = Vec::new();
    for m in models {
        for (h, u) in [
            (None, usage),
            (Some("claude"), usage),
            (Some("codex"), codex),
            (Some("copilot"), usage),
        ] {
            rows.push(json!({"model": m, "harness": h, "usage": u, "usd": pricing::estimate_usd(m, &u, h)}));
        }
    }
    let mut sum = usage;
    sum.add(&codex);
    check(
        "pure/pricing",
        &json!({"estimates": rows, "usage": usage, "codexUsage": codex, "sum": sum, "sumTotal": sum.total()}),
    );
}

#[test]
fn graph_walks() {
    let mut rows = Vec::new();
    for s in world().synthetic.iter().filter(|s| {
        ["succeeded", "map-parallel", "diag-model", "codex-estimate"].contains(&s.name.as_str())
    }) {
        let data = super::projection_goldens::run_data(s);
        rows.push(json!({"run": s.name, "projection": projection::graph_nodes(&data.admitted["graph"], &data.admitted["runtime"]), "harness": harness::graph_nodes(&data.admitted["graph"], Some(&data.admitted["runtime"]))}));
    }
    let pb = &world().pb;
    let graph: Value =
        serde_json::from_str(&std::fs::read_to_string(pb.join("harness/graph.json")).unwrap())
            .unwrap();
    let runtime: Value =
        serde_json::from_str(&std::fs::read_to_string(pb.join("harness/runtime.json")).unwrap())
            .unwrap();
    rows.push(json!({"run": "pb-harness", "projection": projection::graph_nodes(&graph, &runtime), "harness": harness::graph_nodes(&graph, Some(&runtime)), "harnessNoRuntime": harness::graph_nodes(&graph, None)}));
    check("pure/graph-nodes", &json!(rows));
}

#[test]
fn harness_files_and_previews() {
    let pb = &world().pb;
    let files = harness::list_harness_files(pb);
    let defaults = harness::default_paths(pb);
    let previews: Vec<Value> = [
        ("harness/graph.json", "harness/runtime.json", None),
        ("harness/graph.json", "harness/runtime.json", Some("claude-sonnet-4-5")),
        ("harness/nope.json", "harness/runtime.json", None),
        ("/etc/passwd", "harness/runtime.json", None),
        ("../outside.json", "harness/runtime.json", None),
    ]
    .iter()
    .map(|(g, r, m)| json!({"graph": g, "runtime": r, "model": m, "preview": harness::preview(pb, g, r, *m)}))
    .collect();
    let resolved: Vec<Value> = [
        "harness/graph.json",
        "../x",
        "/abs",
        "",
        "harness/../harness/graph.json",
    ]
    .iter()
    .map(|p| {
        json!([
            p,
            harness::resolve_in_root(pb, p).map(|p| p.display().to_string())
        ])
    })
    .collect();
    check(
        "pure/harness",
        &json!({"files": files, "defaults": defaults, "previews": previews, "resolve": resolved}),
    );
}

#[test]
fn config_parsing() {
    let dir = world().root.join("config-cases");
    let cases = [
        ("empty", ""),
        ("pb-only", "pb_root = \"/data/pb\"\n"),
        (
            "pb-command",
            "pb_root = \"/data/pb\"\npb_command = \"uv run --project /x pb\"\n",
        ),
        (
            "sources",
            "[[source]]\nname = \"ci\"\nstate_dir = \"/mnt/ci\"\nactions = \"observe\"\n\n[[source]]\nname = \"local\"\nstate_dir = \"/x\"\n\n[[source]]\nname = \"ci\"\nstate_dir = \"/y\"\n",
        ),
        (
            "pb-and-pb-source",
            "pb_root = \"/data/pb\"\n[[source]]\nname = \"pb\"\nstate_dir = \"/z\"\n",
        ),
        (
            "zsf",
            "zsf_state_dir = \"/srv/zsf\"\nzsf_bin = \"/usr/bin/zsf\"\n",
        ),
        ("invalid", "pb_root = [\n"),
        ("unknown-field", "colour = \"blue\"\n"),
    ];
    let mut rows = Vec::new();
    for (name, text) in cases {
        let d = dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("config.toml"), text).unwrap();
        let c = config::load(&d);
        rows.push(json!({
            "case": name,
            "sources": c.sources.iter().map(|t| json!({"id": t.id, "name": t.name, "kind": t.kind, "roots": t.roots, "actionsAllowed": t.actions_allowed})).collect::<Vec<_>>(),
            "pbRoot": c.pb_root, "pbCommand": c.pb_command(), "zsfStateDir": c.zsf_state_dir, "zsfBin": c.zsf_bin, "issue": c.issue,
        }));
    }
    let missing = config::load(&dir.join("does-not-exist"));
    rows.push(json!({"case": "missing", "sources": missing.sources.len(), "issue": missing.issue}));
    check("pure/config", &json!(rows));
}

#[test]
fn run_context() {
    let w = world();
    let providers = context::providers(Some(&w.pb), Some(&w.zsf));
    let mut rows = Vec::new();
    for run in super::api::run_ids_of_pb() {
        let c = rusqlite::Connection::open_with_flags(
            w.pb.join("pb.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let zid: Option<String> = c
            .query_row(
                "SELECT zeroshot_run_id FROM runs WHERE run_id=?1",
                [&run],
                |r| r.get(0),
            )
            .unwrap();
        if let Some(zid) = zid {
            rows.push(json!({"pbRun": run, "zeroshotRun": zid, "context": context::lookup(&providers, &zid)}));
        }
    }
    for s in &w.synthetic {
        if let Some(ctx) = context::lookup(&providers, &s.run_id) {
            rows.push(json!({"scenario": s.name, "context": ctx}));
        }
    }
    rows.push(
        json!({"unknown": context::lookup(&providers, "01a1a5e8-c91b-7073-b3e9-fdbd46260000")}),
    );
    check("pure/run-context", &json!(rows));
}

#[test]
fn small_helpers() {
    let ids = [
        "01a1f8c8-2984-7640-bc40-aaa51b13aa24",
        "0",
        "",
        "zzzzzzzz-zzzz",
        "01a1f8c8",
    ];
    let cmdlines = [
        (
            "zeroshot\0__zeroshot-run-controller\0--bootstrap\0/s/runs/RID/controller.bootstrap.json",
            "RID",
        ),
        ("zeroshot\0status\0RID", "RID"),
        (
            "zeroshot\0__zeroshot-run-controller\0--bootstrap\0/s/runs/OTHER/x",
            "RID",
        ),
    ];
    let iso = [
        "2026-11-04T10:12:31.984Z",
        "2026-11-04T10:12:31Z",
        "2026-11-04T12:12:31.984+02:00",
        "2026-11-04",
        "garbage",
        "",
    ];
    check(
        "pure/helpers",
        &json!({
            "uuidV7Millis": ids.iter().map(|i| json!([i, source::uuid_v7_millis(i)])).collect::<Vec<_>>(),
            "isControllerOf": cmdlines.iter().map(|(c, r)| json!([c, r, source::is_controller_of(c, r)])).collect::<Vec<_>>(),
            "parseIsoMillis": iso.iter().map(|t| json!([t, transcript::parse_iso_millis(t)])).collect::<Vec<_>>(),
            "steerBody": (["Message from the user, sent while you were working. They read: x\nbody", "plain", "Message from the user, sent while you were working"].iter().map(|t| json!([t, transcript::steer_body(t)])).collect::<Vec<_>>()),
            "claudeProjectSlug": (["/work/a b/c.d", "/home/user/x_y"].iter().map(|t| json!([t, transcript::claude_project_slug(t)])).collect::<Vec<_>>()),
            "validRunId": (["01a1f8c8-2984-7640-bc40-aaa51b13aa24", "-01a1f8c8-2984-7640-bc40-aaa51b13aa2", "x"].iter().map(|t| json!([t, crate::server::valid_run_id(t).is_ok()])).collect::<Vec<_>>()),
            "localRunIds": source::local_run_ids(&world().observe),
        }),
    );
}
