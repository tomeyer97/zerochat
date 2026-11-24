//! pb's database, `<pb_root>/pb.sqlite`, opened read-only. pb ("a ProgramBench loop for
//! Zeroshot graphs") is the separate benchmarking tool behind the Benchmarks page and the
//! benchmark context of a run; zerochat never writes to its database.
//!
//! Its schema has versions: an older pb has no `batches` table (its runs are grouped by
//! `runs.label` instead, the same fallback `pb ls` uses) and lacks some later columns. Readers
//! probe with `has_table` / `has_column` and accept both.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

/// The id (and name) of the run source `pb_root` adds: every pb run's Zeroshot state directory.
pub const SOURCE_ID: &str = "pb";

/// Where pb keeps each run's Zeroshot state directory, as a glob under `<pb_root>`.
pub const RUN_STATE_DIRS: &str = "runs/*/agent/zeroshot-state";

/// How long a read waits for pb's writer before it gives up (pb writes in short transactions).
pub const BUSY_TIMEOUT: Duration = Duration::from_millis(500);

pub fn path(pb_root: &Path) -> PathBuf {
    pb_root.join("pb.sqlite")
}

/// Opens `pb.sqlite` (the file itself, see `path`) read-only.
pub fn open_read_only(db: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let _ = conn.busy_timeout(BUSY_TIMEOUT);
    Ok(conn)
}

pub fn has_table(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

pub fn has_column(conn: &Connection, table: &str, col: &str) -> bool {
    columns(conn, table).is_ok_and(|names| names.iter().any(|name| name == col))
}

/// The column names of `table` (none for a table that does not exist).
fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    stmt.query_map([], |row| row.get::<_, String>(1))?.collect()
}
