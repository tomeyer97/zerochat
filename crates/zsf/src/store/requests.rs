//! The request queue: how the CLI and zerochat ask the writer (the daemon, or a CLI command while
//! no daemon runs) to change a feature. This is the boundary other tools use (through
//! `daemon::submit`); they never write `state.json` themselves.
//!
//! ```text
//! requests/<id>.json        a queued [`Request`], written atomically by the asker
//! requests/done/<id>.json   its [`RequestResult`], written by the writer, removed when read
//! ```
//!
//! Ids start with the time a request was made, so the writer applies them oldest first.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::Result;

use super::{Feature, write_atomic};
use crate::model::{Action, Request, RequestResult};
use crate::time::now_ms;

/// Answers older than this are removed when the next one is written.
pub const STALE_ANSWER_SECS: u64 = 3600;

/// Tells apart requests one process queues within the same millisecond.
static REQUEST_SEQ: AtomicU64 = AtomicU64::new(0);

/// Requests and answers are JSON files with this extension.
const EXTENSION: &str = "json";

fn is_request_file(path: &Path) -> bool {
    path.extension().is_some_and(|x| x == EXTENSION)
}

impl Feature {
    pub fn requests_dir(&self) -> PathBuf {
        self.dir.join("requests")
    }

    /// Where the writer puts its answers.
    fn done_dir(&self) -> PathBuf {
        self.requests_dir().join("done")
    }

    /// Queues a request; returns its id.
    pub fn queue_request(&self, action: Action) -> Result<String> {
        let dir = self.requests_dir();
        fs::create_dir_all(&dir)?;
        let ts = now_ms();
        let seq = REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let id = format!("{ts:013}-{}-{seq}", std::process::id());
        let request = Request {
            id: id.clone(),
            ts,
            action,
        };
        // Written under a temporary name so the writer never reads half a request.
        write_atomic(
            &dir.join(format!("{id}.{EXTENSION}")),
            serde_json::to_string(&request)?.as_bytes(),
        )?;
        Ok(id)
    }

    /// Queued requests, oldest first.
    pub fn pending_requests(&self) -> Vec<(PathBuf, Result<Request>)> {
        let mut paths: Vec<PathBuf> = fs::read_dir(self.requests_dir())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_request_file(p) && p.is_file())
            .collect();
        paths.sort();
        paths
            .into_iter()
            .map(|p| {
                let parsed = fs::read_to_string(&p)
                    .map_err(anyhow::Error::from)
                    .and_then(|t| serde_json::from_str(&t).map_err(anyhow::Error::from));
                (p, parsed)
            })
            .collect()
    }

    pub fn has_pending_requests(&self) -> bool {
        fs::read_dir(self.requests_dir())
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| is_request_file(&e.path()))
    }

    /// Writes the answer to the request at `path` and removes the request.
    pub fn answer_request(&self, path: &Path, result: &RequestResult) -> Result<()> {
        let done = self.done_dir();
        fs::create_dir_all(&done)?;
        // Nobody waits for some answers (`zsf start` queues its request and goes): drop the old.
        let stale = SystemTime::now() - Duration::from_secs(STALE_ANSWER_SECS);
        for entry in fs::read_dir(&done).into_iter().flatten().flatten() {
            if entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t < stale)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        write_atomic(
            &done.join(format!("{}.{EXTENSION}", result.id)),
            serde_json::to_string(result)?.as_bytes(),
        )?;
        fs::remove_file(path).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })?;
        Ok(())
    }

    /// The answer to request `id`, once written (read once: the answer file is removed).
    pub fn request_result(&self, id: &str) -> Option<RequestResult> {
        let path = self.done_dir().join(format!("{id}.{EXTENSION}"));
        let text = fs::read_to_string(&path).ok()?;
        let result = serde_json::from_str(&text).ok()?;
        let _ = fs::remove_file(path);
        Some(result)
    }
}
