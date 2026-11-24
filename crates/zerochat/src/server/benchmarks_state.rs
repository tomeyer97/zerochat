//! What the Benchmarks API keeps between requests: pb query results, reused while `pb.sqlite`
//! is unchanged, and the batches this server started. (pb, the ProgramBench runner, records its
//! batches and runs in `<pb_root>/pb.sqlite`.)

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde_json::Value;
use tokio::sync::Mutex;

use crate::benchmarks::{self, StartedHere};

use super::error::{ApiError, ApiResult};
use super::state::AppState;

/// More cached query results than this and the cache starts over.
const MAX_CACHED_QUERIES: usize = 128;
/// How long a batch this server started is remembered: long enough for pb to write its row.
const STARTED_BATCH_KEPT_FOR: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub(crate) struct BenchmarksState {
    /// Query results by cache key, valid while `pb.sqlite`'s `DbFingerprint` is unchanged.
    queries: Mutex<HashMap<String, (DbFingerprint, Value)>>,
    /// Batches this server started (`pb run --detach`), by id, so the batch page can say
    /// "starting" before pb has written the batch's row.
    started: std::sync::Mutex<HashMap<String, StartedHere>>,
}

impl BenchmarksState {
    pub(crate) fn remember_started(&self, id: String, pid: Option<u32>) {
        if let Ok(mut started) = self.started.lock() {
            started.retain(|_, s| s.at.elapsed().unwrap_or_default() < STARTED_BATCH_KEPT_FOR);
            started.insert(
                id,
                StartedHere {
                    at: SystemTime::now(),
                    pid,
                },
            );
        }
    }

    pub(crate) fn started(&self, id: &str) -> Option<StartedHere> {
        self.started
            .lock()
            .ok()
            .and_then(|started| started.get(id).copied())
    }
}

/// `pb.sqlite`'s state on disk, compared without opening the database: any change in it means
/// the cached answers may be stale.
/// - the main file's mtime and size;
/// - the `-wal` file's mtime and size, or its absence: in WAL mode (pb's default) writes land
///   there, and the main file only changes at a checkpoint;
/// - how many batches are `running` and how many of those still have a live process: a dead
///   runner writes nothing to disk (`benchmarks::running_batches_alive`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DbFingerprint {
    main: Option<(SystemTime, u64)>,
    wal: Option<(SystemTime, u64)>,
    /// `(running, alive)`; `None` when no batch is running.
    running_alive: Option<(u32, u32)>,
}

fn stat_fingerprint(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// `None` when the main database file itself cannot be stat'd (not found, no permission): the
/// caller then does not cache the answer.
pub(crate) fn db_fingerprint(pb_root: &Path) -> Option<DbFingerprint> {
    let main = stat_fingerprint(&pb_root.join("pb.sqlite"))?;
    let wal = stat_fingerprint(&pb_root.join("pb.sqlite-wal"));
    let running_alive = benchmarks::running_batches_alive(pb_root);
    Some(DbFingerprint {
        main: Some(main),
        wal,
        running_alive,
    })
}

impl AppState {
    /// Runs a pb query only when `pb.sqlite`'s fingerprint has changed since the last call under
    /// this `cache_key`; otherwise serves the cached result. A page the UI polls every few
    /// seconds while a batch runs costs a few `stat()` calls per poll instead of SQL queries.
    pub(crate) async fn cached_json<F>(
        &self,
        cache_key: String,
        pb_root: &Path,
        compute: F,
    ) -> ApiResult<Value>
    where
        F: FnOnce(&Path) -> anyhow::Result<Value> + Send + 'static,
    {
        let queries = &self.benchmarks.queries;
        let fp = db_fingerprint(pb_root);
        if let Some(fp) = fp
            && let Some((cached_fp, value)) = queries.lock().await.get(&cache_key)
            && *cached_fp == fp
        {
            return Ok(value.clone());
        }
        let root = pb_root.to_owned();
        let value = tokio::task::spawn_blocking(move || compute(&root))
            .await
            .map_err(|e| ApiError::internal(e.into()))?
            .map_err(ApiError::internal)?;
        if let Some(fp) = fp {
            let mut cache = queries.lock().await;
            if cache.len() > MAX_CACHED_QUERIES {
                cache.clear();
            }
            cache.insert(cache_key, (fp, value.clone()));
        }
        Ok(value)
    }
}
