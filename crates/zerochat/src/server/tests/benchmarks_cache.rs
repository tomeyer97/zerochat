//! The pb query cache: a change pb makes to its database, or a batch runner dying, is seen.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::test_state;

/// A compute closure for `cached_json` that counts its calls in `calls` and answers `value`.
fn counting_compute(
    calls: &Arc<AtomicI64>,
    value: Value,
) -> impl FnOnce(&std::path::Path) -> anyhow::Result<Value> + Send + 'static {
    let calls = calls.clone();
    move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(value)
    }
}

/// A write that only touches `pb.sqlite-wal` (how pb, in `PRAGMA journal_mode=WAL`, actually
/// writes) must invalidate `cached_json`'s cache, not keep serving the first poll's snapshot for
/// the rest of a live batch.
#[tokio::test]
async fn benchmarks_cache_invalidates_on_a_wal_only_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path().to_path_buf());
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("pb.sqlite"), b"main-bytes").unwrap();
    let calls = Arc::new(AtomicI64::new(0));

    let v1 = state
        .cached_json("k".into(), &root, counting_compute(&calls, json!(1)))
        .await
        .unwrap();
    assert_eq!(v1, json!(1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Nothing on disk changed: served from cache, the compute closure does not run again.
    let v2 = state
        .cached_json("k".into(), &root, counting_compute(&calls, json!(2)))
        .await
        .unwrap();
    assert_eq!(v2, json!(1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // A write that only touches `-wal` (the main file's mtime/size are untouched, exactly
    // like a running pb batch under WAL mode): must be seen as a change.
    std::thread::sleep(Duration::from_millis(5));
    std::fs::write(root.join("pb.sqlite-wal"), b"wal-bytes").unwrap();
    let v3 = state
        .cached_json("k".into(), &root, counting_compute(&calls, json!(3)))
        .await
        .unwrap();
    assert_eq!(v3, json!(3));
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    // A second write that grows -wal further: seen as a change again.
    std::thread::sleep(Duration::from_millis(5));
    std::fs::write(root.join("pb.sqlite-wal"), b"wal-bytes-grown").unwrap();
    let v4 = state
        .cached_json("k".into(), &root, counting_compute(&calls, json!(4)))
        .await
        .unwrap();
    assert_eq!(v4, json!(4));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

/// Starting a batch, then killing its runner pid, left the cached batch list/detail JSON
/// saying "running" — a dead pid writes
/// nothing to pb.sqlite, so a fingerprint keyed only on the file's own mtime/size (even once
/// WAL-aware) never saw the change. `db_fingerprint` also asks `running_batches_alive`, which
/// is exactly this: a live pid check with no DB write involved at all.
#[tokio::test]
async fn benchmarks_cache_invalidates_when_a_running_batchs_pid_dies_with_no_db_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(dir.path().to_path_buf());
    let root = dir.path().to_path_buf();

    let conn = rusqlite::Connection::open(root.join("pb.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE batches (id TEXT PRIMARY KEY, status TEXT NOT NULL, pid INTEGER, host TEXT);
         CREATE INDEX ix_batches_status ON batches(status);",
    )
    .unwrap();
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn a short-lived process to be the batch's pid");
    let host = crate::source::hostname();
    conn.execute(
        "INSERT INTO batches (id, status, pid, host) VALUES ('b1','running',?1,?2)",
        rusqlite::params![child.id() as i64, host],
    )
    .unwrap();
    drop(conn);

    let calls = Arc::new(AtomicI64::new(0));
    let v1 = state
        .cached_json(
            "k".into(),
            &root,
            counting_compute(&calls, json!("running")),
        )
        .await
        .unwrap();
    assert_eq!(v1, json!("running"));

    // Kill the pid, but touch nothing in pb.sqlite: the only thing that changed is whether
    // that pid still exists.
    child.kill().expect("kill the spawned process");
    child.wait().unwrap();

    let v2 = state
        .cached_json(
            "k".into(),
            &root,
            counting_compute(&calls, json!("crashed")),
        )
        .await
        .unwrap();
    assert_eq!(
        v2,
        json!("crashed"),
        "a dead pid must invalidate the cache even with zero writes to pb.sqlite"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
