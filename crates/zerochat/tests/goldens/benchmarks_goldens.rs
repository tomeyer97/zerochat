//! benchmarks (pb data → batches, matrix, compare, score card, sweeps, harness options) called
//! directly, plus the pending ("starting") batch and live-cell fill that the API only shows
//! while a batch runs. Plus every `/api/benchmarks/*` route of a zerochat with no pb configured.

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

use super::check;
use super::world::world;
use crate::actions::Cli;
use crate::benchmarks;
use crate::server::{self, AppState, HostAllowlist};

fn ok<T: serde::Serialize>(r: anyhow::Result<T>) -> Value {
    match r {
        Ok(v) => serde_json::to_value(v).unwrap(),
        Err(e) => json!({"error": format!("{e:#}")}),
    }
}

#[test]
fn direct_readers() {
    let pb = &world().pb;
    let batches = benchmarks::list_batches(pb).unwrap();
    let ids: Vec<String> = batches
        .iter()
        .map(|b| {
            serde_json::to_value(b).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let details: Vec<Value> = ids
        .iter()
        .map(|id| json!({"id": id, "detail": ok(benchmarks::batch_detail(pb, id))}))
        .collect();
    let mut compares = Vec::new();
    for a in &ids {
        for b in &ids {
            if a < b {
                compares.push(
                    json!({"a": a, "b": b, "compare": ok(benchmarks::compare_batches(pb, a, b))}),
                );
            }
        }
    }
    check(
        "benchmarks/direct/batches",
        &json!({"batches": batches, "runningAlive": benchmarks::running_batches_alive(pb)}),
    );
    check("benchmarks/direct/details", &json!(details));
    check("benchmarks/direct/compare-all-pairs", &json!(compares));
    check("benchmarks/direct/sweeps", &ok(benchmarks::list_sweeps(pb)));
    check(
        "benchmarks/direct/harness-options",
        &ok(benchmarks::harness_options(pb)),
    );
    let scores: Vec<Value> = super::api::run_ids_of_pb().iter().map(|r| json!({"run": r, "score": ok(benchmarks::run_score(pb, r)), "ended": benchmarks::pb_run_ended(pb, r)})).collect();
    check("benchmarks/direct/scores", &json!(scores));
}

#[test]
fn pending_and_live() {
    let pb = &world().pb;
    let now = UNIX_EPOCH + Duration::from_millis(super::world::NOW_MS);
    let started = benchmarks::StartedHere {
        at: now - Duration::from_secs(20),
        pid: Some(4_194_303),
    };
    let pending = benchmarks::pending_batch(pb, "20991231-000000-abcdef", Some(started), now);
    let unknown = benchmarks::pending_batch(pb, "20991231-000000-abcdef", None, now);
    let mut live = Vec::new();
    for b in benchmarks::list_batches(pb).unwrap() {
        let id = serde_json::to_value(&b).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        if let Ok(Some(d)) = benchmarks::batch_detail(pb, &id) {
            let mut v = serde_json::to_value(d).unwrap();
            let before = v.clone();
            benchmarks::fill_live_cells(pb, &mut v);
            if v != before {
                live.push(json!({"id": id, "detail": v}));
            }
        }
    }
    // read_dir's order is the filesystem's: sort, or the golden depends on the machine.
    let mut runs: Vec<_> = std::fs::read_dir(pb.join("runs"))
        .unwrap()
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.join("agent/zeroshot-state").is_dir())
        .collect();
    runs.sort();
    let roots: Vec<Value> = runs
        .into_iter()
        .map(|p| {
            let root = p.join("agent/zeroshot-state");
            json!({"stateRoot": root, "pbRun": benchmarks::pb_run_of_state_root(pb, &root)})
        })
        .collect();
    check(
        "benchmarks/direct/pending-and-live",
        &json!({"pending": pending, "unknownPending": unknown, "liveFilled": live, "stateRoots": roots}),
    );
}

/// Without `pb_root` every benchmarks route answers 404 `pb_not_configured` (the web's cue for
/// its neutral "Benchmarks needs pb" page), GET and POST alike, before any request validation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn routes_without_pb() {
    let w = world();
    let no_config = tempfile::tempdir().unwrap();
    let state = AppState::new(
        w.state.clone(),
        w.zeroshot_config.clone(),
        no_config.path().to_owned(),
        Cli {
            program: w.root.join("no-such-zeroshot"),
        },
        HostAllowlist::any(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, server::router(Arc::new(state)))
            .await
            .unwrap();
    });
    let client = reqwest::Client::new();
    let answer = |response: reqwest::Response| async move {
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        let body = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
        json!({"status": status, "body": body})
    };
    let base = format!("http://{addr}");
    for (name, path) in [
        ("batches", "/api/benchmarks/batches"),
        ("batch", "/api/benchmarks/batches/20990101-000000-000000"),
        ("compare", "/api/benchmarks/compare?a=x&b=y"),
        ("start-options", "/api/benchmarks/start-options"),
        ("slots", "/api/benchmarks/slots"),
        ("harness-preview", "/api/benchmarks/harness-preview"),
        ("sweeps", "/api/benchmarks/sweeps"),
        ("sweep-report", "/api/benchmarks/sweep-report?label=x"),
        ("score", "/api/benchmarks/runs/20990101-000000-000000/score"),
    ] {
        let response = client.get(format!("{base}{path}")).send().await.unwrap();
        check(
            &format!("api/benchmarks/without-pb/{name}"),
            &answer(response).await,
        );
    }
    for (name, path, body) in [
        (
            "post-start",
            "/api/benchmarks/batches",
            r#"{"tasks":"t","seeds":1,"label":"x"}"#,
        ),
        (
            "post-stop",
            "/api/benchmarks/batches/20990101-000000-000000/stop",
            "{}",
        ),
        (
            "post-write-runtime",
            "/api/benchmarks/sweep-report/write-runtime",
            r#"{"path":"runtime.json"}"#,
        ),
    ] {
        let response = client
            .post(format!("{base}{path}"))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .unwrap();
        check(
            &format!("api/benchmarks/without-pb/{name}"),
            &answer(response).await,
        );
    }
}
