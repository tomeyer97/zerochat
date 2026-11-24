//! Every GET route of the real router (server::router), served on a loopback port against the
//! fixture world, each response pinned as `{status, body}` under testdata/golden/api/. The web
//! model goldens (web/src/*.golden.test.ts) read these same files, so the two sides are pinned
//! together.
//!
//! Not here: `GET /api/runs/{target}/{run}/events` (a server-sent-event stream that only says
//! "changed"; its payload is the run view pinned below) and the static UI fallback (its bytes
//! are the built web bundle). POST routes run the zeroshot/pb/zsf programs; their request
//! validation is pinned in `post_validation`.

use std::sync::Arc;

use serde_json::{Value, json};

use super::world::{NOW_MS, world};
use crate::actions::Cli;
use crate::server::{self, AppState, HostAllowlist};

pub struct Api {
    base: String,
    client: reqwest::Client,
}

impl Api {
    pub async fn start() -> Api {
        crate::clock::test_clock::set(NOW_MS);
        let w = world();
        let mut state = AppState::new(
            w.state.clone(),
            w.zeroshot_config.clone(),
            w.zerochat_config.clone(),
            Cli {
                program: w.root.join("no-such-zeroshot"),
            },
            HostAllowlist::any(),
        );
        state.home = w.home.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, server::router(Arc::new(state)))
                .await
                .unwrap();
        });
        Api {
            base: format!("http://{addr}"),
            client: reqwest::Client::new(),
        }
    }

    pub async fn get(&self, path: &str) -> Value {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        let body = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
        json!({"status": status, "body": body})
    }

    pub async fn post(&self, path: &str, body: &str) -> Value {
        let response = self
            .client
            .post(format!("{}{path}", self.base))
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        let body = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
        json!({"status": status, "body": body})
    }

    /// GET and compare with `api/<name>`.
    pub async fn pin(&self, name: &str, path: &str) -> Value {
        let v = self.get(path).await;
        super::check(&format!("api/{name}"), &v);
        v
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overview() {
    let api = Api::start().await;
    api.pin("overview", "/api/overview").await;
    api.pin("overview-nav", "/api/overview?view=nav").await;
}

/// `GET /api/setup` on the fixture world: no zeroshot program (`no-such-zeroshot`), no zsf
/// program, pb configured. zerochat's version and the counts the other areas' fixtures decide
/// (runs, features) are checked here and shown as placeholders, so adding a run or a feature to
/// the world does not change this golden.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setup() {
    let api = Api::start().await;
    let w = world();
    let mut v = api.get("/api/setup").await;
    let body = &mut v["body"];
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    body["version"] = json!("<version>");
    let runs = std::fs::read_dir(w.state.join("runs"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().join(crate::paths::LEDGER_FILE).is_file())
        .count();
    assert!(runs > 0);
    assert_eq!(body["runs"]["count"], runs);
    body["runs"]["count"] = json!("<runs in the world>");
    assert_eq!(
        body["zsf"]["features"],
        crate::features::names(&w.zsf).len()
    );
    body["zsf"]["features"] = json!("<features in the world>");
    super::check("api/setup", &v);
    let mut probe = api.get("/api/setup?probe").await;
    assert_eq!(probe["body"]["version"], env!("CARGO_PKG_VERSION"));
    probe["body"]["version"] = json!("<version>");
    super::check("api/setup-probe", &probe);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runs_and_transcripts() {
    let api = Api::start().await;
    let w = world();
    for s in w.synthetic.iter().chain(&w.recorded) {
        let dir = format!(
            "runs/{}{}",
            super::family_prefix(&s.tags),
            s.name.replace('/', "-")
        );
        let view = api
            .pin(
                &format!("{dir}/run"),
                &format!("/api/runs/{}/{}", s.target, s.run_id),
            )
            .await;
        for e in view["body"]["executions"].as_array().into_iter().flatten() {
            let id = e["id"].as_u64().unwrap();
            let t = api
                .pin(
                    &format!("{dir}/transcript-{id}"),
                    &format!("/api/runs/{}/{}/transcript/{id}", s.target, s.run_id),
                )
                .await;
            if t["body"]["transcript"]["source"] == "native" {
                api.pin(
                    &format!("{dir}/transcript-{id}-ledger"),
                    &format!(
                        "/api/runs/{}/{}/transcript/{id}?source=ledger",
                        s.target, s.run_id
                    ),
                )
                .await;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_errors() {
    let api = Api::start().await;
    let ok = &world().scenario("succeeded").run_id;
    api.pin("errors/unknown-target", &format!("/api/runs/nowhere/{ok}"))
        .await;
    api.pin("errors/invalid-run-id", "/api/runs/local/not-a-run")
        .await;
    api.pin(
        "errors/run-not-found",
        "/api/runs/local/01a1a5e8-c91b-7073-b3e9-fdbd46260000",
    )
    .await;
    api.pin(
        "errors/observe-run-not-found",
        "/api/runs/observe/01a1a5e8-c91b-7073-b3e9-fdbd46260000",
    )
    .await;
    api.pin(
        "errors/execution-not-found",
        &format!("/api/runs/local/{ok}/transcript/99"),
    )
    .await;
    api.pin(
        "errors/checkpoints-without-zeroshot",
        &format!("/api/runs/local/{ok}/checkpoints"),
    )
    .await;
    api.pin("errors/unknown-api-route", "/api/nope").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn workspace_views() {
    let api = Api::start().await;
    let w = world();
    let ok = &w.scenario("succeeded").run_id;
    let (base, done) = &w.succeeded_commits;
    let r = format!("/api/runs/local/{ok}");
    api.pin("workspace/changes", &format!("{r}/changes")).await;
    api.pin(
        "workspace/changes-execution",
        &format!("{r}/changes?from={base}&to={done}"),
    )
    .await;
    api.pin(
        "workspace/changes-missing-base",
        &format!("{r}/changes?from=0123456789abcdef0123456789abcdef01234567"),
    )
    .await;
    api.pin("workspace/diff", &format!("{r}/diff")).await;
    api.pin(
        "workspace/diff-execution",
        &format!("{r}/diff?from={base}&to={done}"),
    )
    .await;
    api.pin(
        "workspace/diff-path",
        &format!("{r}/diff?path=textkit/core.py"),
    )
    .await;
    api.pin("workspace/file", &format!("{r}/file?path=textkit/core.py"))
        .await;
    api.pin(
        "workspace/file-at-commit",
        &format!("{r}/file?path=textkit/core.py&commit={base}"),
    )
    .await;
    api.pin("workspace/file-missing", &format!("{r}/file?path=nope.txt"))
        .await;
    api.pin(
        "workspace/file-outside",
        &format!("{r}/file?path=../../etc/passwd"),
    )
    .await;
    api.pin("workspace/file-no-path", &format!("{r}/file"))
        .await;
    api.pin(
        "workspace/file-option-like",
        &format!("{r}/file?path=--help&commit=--all"),
    )
    .await;
    api.pin("workspace/tree", &format!("{r}/tree")).await;
    api.pin(
        "workspace/tree-at-commit",
        &format!("{r}/tree?commit={base}"),
    )
    .await;
    let stalled = &w.scenario("stalled").run_id;
    api.pin(
        "workspace/changes-workspace-gone",
        &format!("/api/runs/local/{stalled}/changes"),
    )
    .await;
    api.pin(
        "workspace/diff-workspace-gone",
        &format!("/api/runs/local/{stalled}/diff"),
    )
    .await;
    let observed = &w.scenario("running-live").run_id;
    for verb in ["changes", "diff", "file?path=a", "tree"] {
        api.pin(
            &format!("workspace/observe-{}", verb.split('?').next().unwrap()),
            &format!("/api/runs/observe/{observed}/{verb}"),
        )
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn benchmarks() {
    let api = Api::start().await;
    let list = api
        .pin("benchmarks/batches", "/api/benchmarks/batches")
        .await;
    let ids: Vec<String> = list["body"]["batches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b["id"].as_str().map(str::to_owned))
        .collect();
    assert!(
        ids.len() >= 6,
        "the pb fixture has real and pseudo batches: {ids:?}"
    );
    for id in &ids {
        api.pin(
            &format!("benchmarks/batch/{}", id.replace([':', '#', '/'], "_")),
            &format!("/api/benchmarks/batches/{}", urlencode(id)),
        )
        .await;
    }
    api.pin(
        "benchmarks/batch/missing",
        "/api/benchmarks/batches/20990101-000000-000000",
    )
    .await;
    api.pin(
        "benchmarks/batch/invalid",
        "/api/benchmarks/batches/..%2Fetc",
    )
    .await;
    for (i, pair) in ids.windows(2).enumerate().take(6) {
        api.pin(
            &format!("benchmarks/compare/{i}"),
            &format!(
                "/api/benchmarks/compare?a={}&b={}",
                urlencode(&pair[0]),
                urlencode(&pair[1])
            ),
        )
        .await;
    }
    api.pin(
        "benchmarks/compare/same",
        &format!("/api/benchmarks/compare?a={0}&b={0}", urlencode(&ids[0])),
    )
    .await;
    api.pin(
        "benchmarks/compare/missing-b",
        &format!("/api/benchmarks/compare?a={}", urlencode(&ids[0])),
    )
    .await;
    let options = api
        .pin("benchmarks/start-options", "/api/benchmarks/start-options")
        .await;
    api.pin("benchmarks/slots", "/api/benchmarks/slots").await;
    api.pin(
        "benchmarks/harness-preview/default",
        "/api/benchmarks/harness-preview",
    )
    .await;
    let graphs: Vec<String> = options["body"]["harnessFiles"]["graphs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g.as_str().or(g["path"].as_str()).map(str::to_owned))
        .collect();
    let runtimes: Vec<String> = options["body"]["harnessFiles"]["runtimes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g.as_str().or(g["path"].as_str()).map(str::to_owned))
        .collect();
    for (i, (g, r)) in graphs
        .iter()
        .zip(runtimes.iter().cycle())
        .enumerate()
        .take(4)
    {
        api.pin(
            &format!("benchmarks/harness-preview/{i}"),
            &format!(
                "/api/benchmarks/harness-preview?graph={}&runtime={}",
                urlencode(g),
                urlencode(r)
            ),
        )
        .await;
    }
    api.pin("benchmarks/harness-preview/model-override", "/api/benchmarks/harness-preview?graph=harness/graph.json&runtime=harness/runtime.json&model=claude-sonnet-4-5").await;
    api.pin(
        "benchmarks/harness-preview/outside-root",
        "/api/benchmarks/harness-preview?graph=../../etc/passwd&runtime=harness/runtime.json",
    )
    .await;
    api.pin(
        "benchmarks/harness-preview/missing-file",
        "/api/benchmarks/harness-preview?graph=harness/nope.json&runtime=harness/runtime.json",
    )
    .await;
    api.pin("benchmarks/sweeps", "/api/benchmarks/sweeps").await;
    api.pin(
        "benchmarks/sweep-report-without-pb",
        "/api/benchmarks/sweep-report?label=x",
    )
    .await;
    let runs = run_ids_of_pb();
    for run in &runs {
        api.pin(
            &format!("benchmarks/score/{run}"),
            &format!("/api/benchmarks/runs/{run}/score"),
        )
        .await;
    }
    api.pin(
        "benchmarks/score/missing",
        "/api/benchmarks/runs/20990101-000000-000000/score",
    )
    .await;
}

pub fn run_ids_of_pb() -> Vec<String> {
    let c = rusqlite::Connection::open_with_flags(
        world().pb.join("pb.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut s = c
        .prepare("SELECT run_id FROM runs ORDER BY run_id")
        .unwrap();
    s.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

pub fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn features() {
    let api = Api::start().await;
    api.pin("features/list", "/api/features").await;
    for name in ["calc", "paused", "broken", "fresh", "missing"] {
        api.pin(
            &format!("features/{name}/detail"),
            &format!("/api/features/{name}"),
        )
        .await;
        api.pin(
            &format!("features/{name}/events"),
            &format!("/api/features/{name}/events"),
        )
        .await;
        api.pin(
            &format!("features/{name}/changes"),
            &format!("/api/features/{name}/changes"),
        )
        .await;
    }
    api.pin(
        "features/calc/changes-diff",
        "/api/features/calc/changes?diff=true",
    )
    .await;
    api.pin(
        "features/calc/events-04",
        "/api/features/calc/events?slice=04",
    )
    .await;
    for (slice, attempt) in [("04", 1), ("04", 2), ("04", 9), ("01", 1), ("99", 1)] {
        api.pin(
            &format!("features/calc/task-{slice}-{attempt}"),
            &format!("/api/features/calc/slices/{slice}/task/{attempt}"),
        )
        .await;
    }
    for slice in ["01", "02", "03", "04", "11", "99"] {
        api.pin(
            &format!("features/calc/commit-{slice}"),
            &format!("/api/features/calc/slices/{slice}/commit"),
        )
        .await;
        api.pin(
            &format!("features/calc/worktree-{slice}"),
            &format!("/api/features/calc/slices/{slice}/worktree"),
        )
        .await;
    }
    for (i, file) in [
        "NOTES.md",
        "gate/out.txt",
        "missing.txt",
        "..%2F..%2Ffeature.toml",
    ]
    .iter()
    .enumerate()
    {
        api.pin(
            &format!("features/calc/artifact-{i}"),
            &format!("/api/features/calc/slices/04/artifacts/{file}"),
        )
        .await;
    }
}

/// POST routes whose answer is decided before any program runs (validation, refusals).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn post_validation() {
    let api = Api::start().await;
    let w = world();
    let local = &w.scenario("succeeded").run_id;
    let observed = &w.scenario("running-live").run_id;
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "steer-observe",
            format!("/api/runs/observe/{observed}/steer"),
            r#"{"execution":1,"message":"hi"}"#,
        ),
        (
            "steer-empty",
            format!("/api/runs/local/{local}/steer"),
            r#"{"execution":1,"message":"   "}"#,
        ),
        (
            "steer-too-long",
            format!("/api/runs/local/{local}/steer"),
            "LONG",
        ),
        (
            "steer-over-256k",
            format!("/api/runs/local/{local}/steer"),
            "LONG256",
        ),
        (
            "steer-control-character",
            format!("/api/runs/local/{local}/steer"),
            r#"{"execution":1,"message":"hi\u0007"}"#,
        ),
        (
            "steer-no-capability",
            format!("/api/runs/local/{local}/steer"),
            r#"{"execution":1,"message":"hi"}"#,
        ),
        (
            "stop-observe",
            format!("/api/runs/observe/{observed}/stop"),
            "{}",
        ),
        (
            "resume-observe",
            format!("/api/runs/observe/{observed}/resume"),
            "{}",
        ),
        (
            "resume-bad-checkpoint",
            format!("/api/runs/local/{local}/resume"),
            r#"{"fromCheckpoint":"-x"}"#,
        ),
        (
            "replay-observe",
            format!("/api/runs/observe/{observed}/replay"),
            r#"{"fromCheckpoint":"c1"}"#,
        ),
        (
            "replay-bad-runtime",
            format!("/api/runs/local/{local}/replay"),
            r#"{"fromCheckpoint":"c1","runtime":{"harness":"nope"}}"#,
        ),
        ("invalid-json", format!("/api/runs/local/{local}/stop"), "{"),
        (
            "feature-unknown-action",
            "/api/features/calc/actions".to_owned(),
            r#"{"action":"explode"}"#,
        ),
        (
            "feature-start-without-zsf",
            "/api/features/calc/actions".to_owned(),
            r#"{"action":"start"}"#,
        ),
        (
            "slice-retry-waiting",
            "/api/features/calc/slices/03/actions".to_owned(),
            r#"{"action":"retry"}"#,
        ),
        (
            "slice-approve-not-checkpoint",
            "/api/features/calc/slices/01/actions".to_owned(),
            r#"{"action":"approve"}"#,
        ),
        (
            "slice-unknown",
            "/api/features/calc/slices/99/actions".to_owned(),
            r#"{"action":"skip"}"#,
        ),
        (
            "feature-missing",
            "/api/features/missing/actions".to_owned(),
            r#"{"action":"stop"}"#,
        ),
        (
            "benchmark-start-invalid",
            "/api/benchmarks/batches".to_owned(),
            r#"{"tasks":[],"seeds":0}"#,
        ),
        (
            "benchmark-stop-invalid",
            "/api/benchmarks/batches/..%2Fx/stop".to_owned(),
            "{}",
        ),
        (
            "write-runtime-invalid",
            "/api/benchmarks/sweep-report/write-runtime".to_owned(),
            r#"{"path":"../../x.json"}"#,
        ),
    ];
    for (name, path, body) in cases {
        let body = match body {
            "LONG" => json!({"execution": 1, "message": "x".repeat(17 * 1024)}).to_string(),
            // Zeroshot 16.1's limit is 256 KiB.
            "LONG256" => json!({"execution": 1, "message": "x".repeat(256 * 1024 + 1)}).to_string(),
            _ => body.to_owned(),
        };
        let v = api.post(&path, &body).await;
        super::check(&format!("api/post/{name}"), &v);
    }
}
