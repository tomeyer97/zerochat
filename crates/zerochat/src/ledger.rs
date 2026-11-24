//! Queries over a Zeroshot ledger (`runs.sqlite3`, see `paths::ledger_path`) for readers
//! outside `source`, which reads whole runs.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

/// How long a quick look waits for Zeroshot's writer before it gives up.
const BUSY_TIMEOUT: Duration = Duration::from_millis(300);

/// The nodes a run's ledger shows started and not completed, in start order; empty once the
/// run reached its terminal event. `None` when the ledger cannot be read (not written yet, or
/// busy).
pub fn live_nodes(ledger: &Path) -> Option<Vec<String>> {
    if !ledger.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(
        ledger,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = conn.busy_timeout(BUSY_TIMEOUT);
    let ended: bool = conn
        .query_row("SELECT EXISTS(SELECT 1 FROM v2_run_events WHERE json_extract(event_json, '$.kind') = 'terminal')", [], |r| r.get(0))
        .ok()?;
    if ended {
        return Some(Vec::new());
    }
    // `node_started` events whose execution has no `node_completed` event yet.
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(event_json, '$.reference.node') FROM v2_run_events \
             WHERE json_extract(event_json, '$.kind') = 'node_started' \
               AND json_extract(event_json, '$.reference.execution') NOT IN ( \
                   SELECT json_extract(event_json, '$.completion.reference.execution') FROM v2_run_events \
                   WHERE json_extract(event_json, '$.kind') = 'node_completed' \
                     AND json_extract(event_json, '$.completion.reference.execution') IS NOT NULL) \
             ORDER BY sequence",
        )
        .ok()?;
    let nodes = stmt
        .query_map([], |r| r.get::<_, Option<String>>(0))
        .ok()?
        .filter_map(|r| r.ok().flatten());
    let mut out: Vec<String> = Vec::new();
    for node in nodes {
        if !out.contains(&node) {
            out.push(node);
        }
    }
    Some(out)
}
