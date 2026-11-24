//! Differential check of the direct-target reader ([`Remote`]): a stub target replays fixed
//! responses, and everything the reader makes of them (listings, run detail, events, errors and
//! the requests it sent) is printed as one text and compared with `EXPECTED`.
//!
//! The file uses only `crate::source`'s public items, so the same file can be compiled into an
//! older checkout (`#[cfg(test)] #[path = "…/remote_replay_tests.rs"] mod remote_replay;` at the
//! end of its `source.rs`) to show both readers print the same text.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::source::{Remote, RunNotFound};

/// The recorded target: request path → (status line, body).
fn response(path: &str) -> (&'static str, String) {
    let ok = |body: Value| ("200 OK", body.to_string());
    match path {
        "/.well-known/zeroshot-native-v2" => {
            ok(json!({"protocol": "zeroshot-native-v2", "version": 1}))
        }
        "/ui/api/runs?" => ok(json!({
            "runs": [
                {"runId": "r1", "title": "One", "phase": "running", "terminal": null, "createdAt": 1795175269036u64,
                 "source": {"kind": "cli"}, "cursor": "v2:3"},
                {"runId": "r2", "phase": "unavailable", "issue": "ledger unreadable", "createdAt": "not a number"},
            ],
            "nextCursor": "page-2",
        })),
        "/ui/api/runs?after=page-2" => ok(json!({
            "runs": [{"runId": "r3", "title": "Three", "phase": "completed", "terminal": {"status": "succeeded"}, "cursor": 7}],
        })),
        "/ui/api/runs/r1" => ok(json!({
            "runId": "r1", "title": "One", "graph": {"nodes": [{"name": "worker"}]}, "initialInput": {"task": "t"},
            "runtime": {"provider": "claude"}, "source": {"kind": "cli"}, "snapshot": {"phase": "running"},
            "extra": "not copied",
        })),
        "/ui/api/runs/r1/history?after=v2:0" => ok(json!({
            "events": [
                {"cursor": "v2:1", "event": {"kind": "node_started", "reference": {"node": "worker", "nodeInstance": "1", "execution": "12"}}},
                {"cursor": "bad", "event": {"kind": "skipped"}},
                {"cursor": "v2:2", "event": {"kind": "safe_log", "execution": "12", "line": "hi", "list": [{"execution": "4"}, {"execution": "x"}]}},
            ],
            "complete": false,
        })),
        "/ui/api/runs/r1/history?after=v2:2" => ok(json!({
            "events": [{"cursor": "v2:3", "event": {"kind": "node_completed", "execution": 12}}],
            "complete": true,
        })),
        "/ui/api/runs/r1/history?after=v2:9" => ok(json!({"events": []})),
        "/ui/api/runs/gone" => (
            "404 Not Found",
            json!({"message": "no such run"}).to_string(),
        ),
        "/ui/api/runs/boom" => (
            "500 Internal Server Error",
            json!({"message": "kaput"}).to_string(),
        ),
        "/ui/api/runs/plain" => ("503 Service Unavailable", "not json".to_owned()),
        _ => (
            "404 Not Found",
            json!({"message": "unknown path"}).to_string(),
        ),
    }
}

/// A target without the UI: every path is 404.
fn no_ui(_path: &str) -> (&'static str, String) {
    ("404 Not Found", "{}".to_owned())
}

/// Serves `respond` on a free local port; returns the origin and the log of requests
/// (`path origin-header`).
fn stub(respond: fn(&str) -> (&'static str, String)) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let log = Arc::new(Mutex::new(Vec::new()));
    let requests = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let path = request_line.split(' ').nth(1).unwrap_or("").to_owned();
            let mut origin_header = String::new();
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("origin")
                {
                    origin_header = value.trim().to_owned();
                }
            }
            requests
                .lock()
                .unwrap()
                .push(format!("{path} origin={origin_header}"));
            let (status, body) = respond(&path);
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (origin, log)
}

fn error_text(error: &anyhow::Error) -> String {
    let not_found = error.downcast_ref::<RunNotFound>().is_some();
    format!(
        "error{}: {error:#}",
        if not_found { " (RunNotFound)" } else { "" }
    )
}

async fn replay() -> String {
    let remote = Remote::new();
    let (origin, log) = stub(response);
    let mut out = Vec::new();
    match remote.probe(&origin).await {
        Ok(v) => out.push(format!("probe: {v}")),
        Err(e) => out.push(format!("probe {}", error_text(&e))),
    }
    match remote.listings(&origin).await {
        Ok(listings) => {
            for listing in listings {
                out.push(format!(
                    "listing: {} cursor={:?}",
                    serde_json::to_string(&listing).unwrap(),
                    listing.cursor
                ));
            }
        }
        Err(e) => out.push(format!("listings {}", error_text(&e))),
    }
    for (run, after) in [("r1", 0), ("r1", 9), ("gone", 0), ("boom", 0), ("plain", 0)] {
        match remote.read(&origin, run, after).await {
            Ok((stored, events)) => {
                out.push(format!("read {run} after {after}: {stored}"));
                for event in events {
                    out.push(format!("  event {}: {}", event.seq, event.body));
                }
            }
            Err(e) => out.push(format!("read {run} after {after} {}", error_text(&e))),
        }
    }
    let (bare, bare_log) = stub(no_ui);
    match remote.listings(&bare).await {
        Ok(listings) => out.push(format!("no-ui listings: {}", listings.len())),
        Err(e) => out.push(format!("no-ui listings {}", error_text(&e))),
    }
    match remote.read(&bare, "r1", 0).await {
        Ok(_) => out.push("no-ui read: ok".to_owned()),
        Err(e) => out.push(format!("no-ui read {}", error_text(&e))),
    }
    // Nothing listens on port 1.
    match remote.probe("http://127.0.0.1:1").await {
        Ok(_) => out.push("unreachable probe: ok".to_owned()),
        Err(e) => out.push(format!("unreachable probe {}", error_text(&e))),
    }
    for line in log
        .lock()
        .unwrap()
        .iter()
        .chain(bare_log.lock().unwrap().iter())
    {
        out.push(format!("request {line}"));
    }
    out.join("\n")
        .replace(&origin, "ORIGIN")
        .replace(&bare, "BARE")
}

const EXPECTED: &str = include_str!("remote_replay_expected.txt");

#[tokio::test]
async fn the_direct_target_reader_reads_recorded_responses_as_before() {
    let output = replay().await;
    if std::env::var_os("PRINT_REMOTE_REPLAY").is_some() {
        println!("{output}");
    }
    assert_eq!(output, EXPECTED.trim_end());
}
