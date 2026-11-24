//! Direct targets: a Zeroshot target's own HTTP API, read the way the target's web UI reads it.
//! The paths, cursors and page limits below are that API's shape; keep them in sync with the
//! target.

use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value};

use super::{Event, RunListing, RunNotFound, str_at};

/// The target's protocol probe (only the replay tests read it; nothing in the server calls it).
#[cfg(test)]
const PROBE_PATH: &str = "/.well-known/zeroshot-native-v2";

/// The run list, one page per request: `<RUNS_PATH>?` for the first page, then
/// `<RUNS_PATH>?after=<nextCursor>`. One run is `<RUNS_PATH>/<run id>`, its events
/// `<RUNS_PATH>/<run id>/history?after=v2:<seq>`.
const RUNS_PATH: &str = "/ui/api/runs";

/// History cursors are `v2:<ledger sequence>`.
const CURSOR_PREFIX: &str = "v2:";

/// At most this many pages of the run list, and of one run's history, are read.
const MAX_LISTING_PAGES: usize = 20;
const MAX_HISTORY_PAGES: usize = 400;

/// The fields of the target's run detail that make up the run's `admitted` definition.
const ADMITTED_FIELDS: [&str; 5] = ["title", "graph", "initialInput", "runtime", "source"];

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

fn listing_page_path(after: Option<&str>) -> String {
    match after {
        Some(after) => format!("{RUNS_PATH}?after={after}"),
        None => format!("{RUNS_PATH}?"),
    }
}

fn run_path(run_id: &str) -> String {
    format!("{RUNS_PATH}/{run_id}")
}

fn history_path(run_id: &str, after: u64) -> String {
    format!("{RUNS_PATH}/{run_id}/history?after={CURSOR_PREFIX}{after}")
}

pub struct Remote {
    client: reqwest::Client,
}

impl Remote {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("http client");
        Self { client }
    }

    async fn get(&self, origin: &str, path: &str) -> Result<Value> {
        let url = format!("{origin}{path}");
        let response = self
            .client
            .get(&url)
            .header("origin", origin)
            .send()
            .await
            .map_err(|error| anyhow!("{} is unreachable: {}", origin, root_cause(&error)))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("no details");
            if status == reqwest::StatusCode::NOT_FOUND
                && path.starts_with(&listing_page_path(None))
            {
                bail!("{origin} does not serve run history (a target built without the UI)");
            }
            if status == reqwest::StatusCode::NOT_FOUND
                && path.starts_with(&format!("{RUNS_PATH}/"))
            {
                return Err(anyhow!(RunNotFound));
            }
            bail!("{origin} answered {status}: {message}");
        }
        Ok(body)
    }

    #[cfg(test)]
    pub async fn probe(&self, origin: &str) -> Result<Value> {
        self.get(origin, PROBE_PATH).await
    }

    pub async fn listings(&self, origin: &str) -> Result<Vec<RunListing>> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MAX_LISTING_PAGES {
            let page = self
                .get(origin, &listing_page_path(after.as_deref()))
                .await?;
            out.extend(
                page["runs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(RunListing::of_target),
            );
            match page["nextCursor"].as_str() {
                Some(next) => after = Some(next.to_owned()),
                None => break,
            }
        }
        Ok(out)
    }

    /// The run detail plus every event after `after`.
    pub async fn read(
        &self,
        origin: &str,
        run_id: &str,
        after: u64,
    ) -> Result<(Value, Vec<Event>)> {
        let detail = self.get(origin, &run_path(run_id)).await?;
        let admitted: Map<String, Value> = ADMITTED_FIELDS
            .iter()
            .map(|&field| (field.to_owned(), detail[field].clone()))
            .collect();
        let stored = serde_json::json!({"admitted": admitted, "snapshot": detail["snapshot"]});
        let mut events = Vec::new();
        let mut cursor = after;
        for _ in 0..MAX_HISTORY_PAGES {
            let page = self.get(origin, &history_path(run_id, cursor)).await?;
            for item in page["events"].as_array().into_iter().flatten() {
                let Some(seq) = item["cursor"].as_str().and_then(sequence_of) else {
                    continue;
                };
                let mut body = item["event"].clone();
                normalize_numbers(&mut body);
                events.push(Event { seq, body });
                cursor = cursor.max(seq);
            }
            // `complete: false`: more events follow this page. Anything else (true, or no field
            // from a target that does not page) means the history is read.
            if page["complete"].as_bool() != Some(false) {
                break;
            }
        }
        Ok((stored, events))
    }
}

impl RunListing {
    /// A run as the target's run list describes it.
    fn of_target(run: &Value) -> Self {
        let text = |key: &str| run.get(key).and_then(Value::as_str).map(str::to_owned);
        RunListing {
            run_id: str_at(run, "/runId"),
            title: str_at(run, "/title"),
            phase: str_at(run, "/phase"),
            terminal: run.get("terminal").cloned().unwrap_or(Value::Null),
            created_at: run.get("createdAt").and_then(Value::as_u64),
            source: run.get("source").cloned().unwrap_or(Value::Null),
            issue: text("issue"),
            cursor: text("cursor"),
        }
    }
}

// ---------- helpers ----------

/// `v2:<seq>` → the ledger sequence.
fn sequence_of(cursor: &str) -> Option<u64> {
    cursor.strip_prefix(CURSOR_PREFIX)?.parse().ok()
}

fn root_cause(error: &reqwest::Error) -> String {
    let mut source: &dyn std::error::Error = error;
    while let Some(next) = source.source() {
        source = next;
    }
    source.to_string()
}

/// The target UI stringifies execution ids so JavaScript keeps their precision; zerochat's
/// projection reads numbers, so turn the known id fields back into numbers.
fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if matches!(key.as_str(), "execution" | "nodeInstance")
                    && let Some(number) = child.as_str().and_then(|s| s.parse::<u64>().ok())
                {
                    *child = Value::from(number);
                    continue;
                }
                normalize_numbers(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_numbers),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_stringified_ids() {
        let mut event = serde_json::json!({"kind":"safe_log","execution":"12","reference":{"nodeInstance":"3","node":"a"}});
        normalize_numbers(&mut event);
        assert_eq!(event["execution"], 12);
        assert_eq!(event["reference"]["nodeInstance"], 3);
        assert_eq!(event["reference"]["node"], "a");
    }
}
