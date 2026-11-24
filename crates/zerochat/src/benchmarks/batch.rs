//! The two kinds of batch: a real `batches` row (`BatchRow`), and a label pseudo-batch
//! (`PseudoBatch`) grouping the runs no batch owns. Their status, and the harness they ran.

use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::db::{get_opt, open_ro, query_runs, unbatched};
use super::run::{HarnessOutcome, RunRow, by_start_time};

/// A batch's status as the list and the batch page show it: pb's `batches.status` reconciled
/// with whether its runner still lives (`reconciled_status`), a label pseudo-batch's status
/// derived from its runs (`pseudo_status`), or a batch pb has not recorded yet (`pending_batch`).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Running,
    Done,
    /// Stopped by a person (`pb stop`, or Ctrl-C).
    Stopped,
    /// pb's runner died while the batch was running.
    Crashed,
    /// Started here, and pb has not recorded the batch yet.
    #[default]
    Starting,
    /// Started here, and pb never recorded the batch (its runner died or stayed silent).
    FailedToStart,
    /// Any other status pb recorded, shown as pb wrote it.
    #[serde(untagged)]
    Other(String),
}

impl BatchStatus {
    pub fn from_pb(status: &str) -> BatchStatus {
        serde_json::from_value(Value::from(status))
            .unwrap_or_else(|_| BatchStatus::Other(status.to_owned()))
    }
}

/// A batch's status with its runner's liveness: a `batches` row left `running` by a runner
/// process that is gone reads as `Crashed`. pb applies the same rule to its own rows the next
/// time any `pb` command runs; zerochat never writes to pb.sqlite, so it applies it on reading.
/// A crashed batch must not read `running`: the Start form refuses while any batch does. A pid
/// recorded on another host cannot be checked from here, so (as pb does) it counts as alive.
pub(super) fn reconciled_status(status: &str, pid: Option<i64>, host: Option<&str>) -> BatchStatus {
    let status = BatchStatus::from_pb(status);
    if status != BatchStatus::Running {
        return status;
    }
    if let Some(h) = host
        && crate::source::hostname().as_deref() != Some(h)
    {
        return BatchStatus::Running;
    }
    let alive = pid.is_some_and(|p| p > 0 && crate::source::process_alive(p as u32));
    if alive {
        BatchStatus::Running
    } else {
        BatchStatus::Crashed
    }
}

/// How many `batches` rows are `status='running'` and how many of those still have a live
/// process; `None` when none is running. pb runs several batches at once, so every running row
/// is checked. Cheap (`ix_batches_status` indexes the lookup, and few rows are ever running),
/// and never cached: a process dying writes nothing to pb.sqlite.
pub fn running_batches_alive(pb_root: &Path) -> Option<(u32, u32)> {
    let conn = open_ro(pb_root).ok()?;
    let mut stmt = conn
        .prepare("SELECT pid, host FROM batches WHERE status='running'")
        .ok()?;
    let rows: Vec<(Option<i64>, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok()?
        .filter_map(Result::ok)
        .collect();
    if rows.is_empty() {
        return None;
    }
    let alive = rows
        .iter()
        .filter(|(pid, host)| {
            reconciled_status("running", *pid, host.as_deref()) == BatchStatus::Running
        })
        .count();
    Some((rows.len() as u32, alive as u32))
}

/// One `batches` row, with whatever columns this database has.
pub(super) struct BatchRow {
    pub(super) id: String,
    pub(super) label: Option<String>,
    pub(super) version_id: Option<String>,
    /// JSON lists: the batch's planned grid.
    pub(super) task_ids: Option<String>,
    pub(super) seeds: Option<String>,
    pub(super) started_at: f64,
    pub(super) ended_at: Option<f64>,
    /// As pb wrote it; see `status()`.
    status: String,
    pid: Option<i64>,
    host: Option<String>,
    pub(super) limits_json: Option<String>,
    pub(super) parallel: Option<i64>,
}

impl BatchRow {
    fn from_row(row: &Row) -> BatchRow {
        BatchRow {
            id: get_opt(row, "id").unwrap_or_default(),
            label: get_opt(row, "label"),
            version_id: get_opt(row, "version_id"),
            task_ids: get_opt(row, "task_ids"),
            seeds: get_opt(row, "seeds"),
            started_at: get_opt(row, "started_at").unwrap_or(0.0),
            ended_at: get_opt(row, "ended_at"),
            status: get_opt(row, "status").unwrap_or_default(),
            pid: get_opt(row, "pid"),
            host: get_opt(row, "host"),
            limits_json: get_opt(row, "limits_json"),
            parallel: get_opt(row, "parallel"),
        }
    }

    pub(super) fn all(conn: &Connection) -> Result<Vec<BatchRow>> {
        let mut stmt = conn.prepare("SELECT * FROM batches")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(BatchRow::from_row(row));
        }
        Ok(out)
    }

    /// `None` when there is no such batch (or no `batches` table).
    pub(super) fn find(conn: &Connection, id: &str) -> Option<BatchRow> {
        conn.query_row("SELECT * FROM batches WHERE id=?1", [id], |r| {
            Ok(BatchRow::from_row(r))
        })
        .ok()
    }

    /// See `reconciled_status`.
    pub(super) fn status(&self) -> BatchStatus {
        reconciled_status(&self.status, self.pid, self.host.as_deref())
    }

    pub(super) fn tasks(&self) -> Vec<String> {
        self.task_ids
            .as_deref()
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default()
    }

    pub(super) fn seed_list(&self) -> Vec<i64> {
        self.seeds
            .as_deref()
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default()
    }
}

/// The runs of one `label` that no batch owns, shown as a batch of their own: `id` is
/// `label:<label>` (`label:` for runs with no label, shown as "(no label)"). On a database with
/// no `batches` table, every run is in one; `pb ls` groups the same way.
pub(super) struct PseudoBatch {
    pub(super) label: Option<String>,
    /// Oldest first; never empty.
    pub(super) runs: Vec<RunRow>,
}

impl PseudoBatch {
    /// Every label's pseudo-batch, in the order of each label's first run.
    pub(super) fn all(conn: &Connection, batch_col: bool) -> Result<Vec<PseudoBatch>> {
        let sql = format!(
            "SELECT * FROM runs WHERE {} ORDER BY started_at",
            unbatched(batch_col)
        );
        let mut out: Vec<PseudoBatch> = Vec::new();
        for run in query_runs(conn, &sql, [])? {
            match out.iter_mut().find(|b| b.label == run.label) {
                Some(batch) => batch.runs.push(run),
                None => out.push(PseudoBatch {
                    label: run.label.clone(),
                    runs: vec![run],
                }),
            }
        }
        Ok(out)
    }

    /// One label's pseudo-batch; `None` when that label has no such runs.
    pub(super) fn find(
        conn: &Connection,
        batch_col: bool,
        label: Option<&str>,
    ) -> Result<Option<PseudoBatch>> {
        let sql = format!(
            "SELECT * FROM runs WHERE {} AND label IS ?1 ORDER BY started_at ASC",
            unbatched(batch_col)
        );
        let runs = query_runs(conn, &sql, [label])?;
        Ok((!runs.is_empty()).then(|| PseudoBatch {
            label: label.map(str::to_owned),
            runs,
        }))
    }

    pub(super) fn id(&self) -> String {
        format!("label:{}", self.label.clone().unwrap_or_default())
    }

    /// When its first run started (Unix seconds).
    pub(super) fn started_at(&self) -> f64 {
        self.runs
            .iter()
            .map(|r| r.started_at)
            .fold(f64::INFINITY, f64::min)
    }

    /// When its last run ended, or started if it has not ended (Unix seconds).
    pub(super) fn ended_at(&self) -> f64 {
        self.runs
            .iter()
            .map(|r| r.ended_at.unwrap_or(r.started_at))
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// The tasks its runs cover, sorted.
    pub(super) fn tasks(&self) -> Vec<String> {
        let mut tasks: Vec<String> = self.runs.iter().map(|r| r.task_id.clone()).collect();
        tasks.sort();
        tasks.dedup();
        tasks
    }

    /// The seeds its runs cover, sorted.
    pub(super) fn seeds(&self) -> Vec<i64> {
        let mut seeds: Vec<i64> = self.runs.iter().map(|r| r.seed).collect();
        seeds.sort();
        seeds.dedup();
        seeds
    }

    /// There is no pb process to reconcile: `running` while any run is, `stopped` / `crashed`
    /// when every run was stopped / crashed and none was scored, else `done` (also when scored
    /// runs are mixed with failed ones).
    pub(super) fn status(&self) -> BatchStatus {
        let all = |outcome: HarnessOutcome| self.runs.iter().all(|r| r.outcome() == outcome);
        let scored = self.runs.iter().any(RunRow::evaluated);
        if self
            .runs
            .iter()
            .any(|r| r.outcome() == HarnessOutcome::Running)
        {
            BatchStatus::Running
        } else if !scored && all(HarnessOutcome::Interrupted) {
            BatchStatus::Stopped
        } else if !scored && all(HarnessOutcome::Crashed) {
            BatchStatus::Crashed
        } else {
            BatchStatus::Done
        }
    }

    /// The harness version of the label's most recent unbatched run.
    pub(super) fn version_id(&self, conn: &Connection, batch_col: bool) -> Option<String> {
        let sql = format!(
            "SELECT version_id FROM runs WHERE {} AND label IS ?1 ORDER BY started_at DESC LIMIT 1",
            unbatched(batch_col)
        );
        conn.query_row(&sql, [self.label.as_deref()], |r| {
            r.get::<_, Option<String>>(0)
        })
        .ok()
        .flatten()
    }

    /// A pseudo-batch has no `batches.limits_json`: its most recent run's caps stand in (in
    /// practice every run of a label comes from the same `pb run`).
    pub(super) fn limits_json(&self) -> Option<String> {
        self.runs
            .iter()
            .max_by(|a, b| by_start_time(a, b))
            .and_then(|r| r.limits_json.clone())
    }
}

/// What tells two harnesses apart: `version` is the short hash of the harness's `version_id`
/// (what changes between a stock run and a variant, or two commits of the harness);
/// `zeroshot_version` is the Zeroshot CLI build it ran under, and `models` the distinct models of
/// its agent nodes (from the recorded `runtime_json`), sorted.
#[derive(Default)]
pub(super) struct HarnessIdentity {
    pub(super) version: Option<String>,
    pub(super) zeroshot_version: Option<String>,
    pub(super) models: Vec<String>,
}

pub(super) fn harness_identity(conn: &Connection, version_id: &Option<String>) -> HarnessIdentity {
    let Some(vid) = version_id.as_deref() else {
        return HarnessIdentity::default();
    };
    let found = conn.query_row(
        "SELECT * FROM harness_versions WHERE version_id=?1",
        [vid],
        |r| {
            Ok((
                get_opt::<String>(r, "zeroshot_version"),
                get_opt::<String>(r, "runtime_json"),
            ))
        },
    );
    let (zeroshot_version, runtime_json) = found.unwrap_or_default();
    let mut models: Vec<String> = runtime_json
        .as_deref()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .and_then(|rt| rt.get("nodes").and_then(Value::as_object).cloned())
        .into_iter()
        .flatten()
        .filter_map(|(_, node)| {
            (node.get("kind").and_then(Value::as_str) == Some("agent"))
                .then(|| node.get("model").and_then(Value::as_str).map(str::to_owned))
                .flatten()
        })
        .collect();
    models.sort();
    models.dedup();
    HarnessIdentity {
        version: Some(vid.chars().take(8).collect()),
        zeroshot_version,
        models,
    }
}
