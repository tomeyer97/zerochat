//! The benchmarks reader's tests, one file per section of the module, and the fixtures they
//! share: a pb.sqlite built from `schema/*.sql`, and `Run` / `Batch` builders for its rows.

mod batches;
mod cells;
mod compare;
mod harness_options;
mod live;
mod replays;
mod score;
mod sweeps;

use std::path::Path;

use rusqlite::{Connection, ToSql};

/// A later pb's addition to pb.sqlite's schema (`schema/<name>.sql`), applied on top of `base.sql`.
const BATCHES: &str = include_str!("schema/batches.sql");
const NODE_DURATIONS: &str = include_str!("schema/node_durations.sql");
const SWEEPS: &str = include_str!("schema/sweeps.sql");
const REPLAYS: &str = include_str!("schema/replays.sql");

/// A pb.sqlite with the columns zerochat reads (`schema/base.sql`), with the `batches` table and
/// `runs.batch_id` when `with_batches`, and harness version `v1`.
fn seed_db(path: &Path, with_batches: bool) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(include_str!("schema/base.sql")).unwrap();
    if with_batches {
        conn.execute_batch(BATCHES).unwrap();
    }
    conn.execute("INSERT INTO harness_versions (version_id, zeroshot_version, git_commit) VALUES ('v1', '11.2.0', 'abc123')", [])
        .unwrap();
    conn
}

/// Adds the columns and tables of one of `schema/*.sql`.
fn migrate(conn: &Connection, schema: &str) {
    conn.execute_batch(schema).unwrap();
}

/// A `runs` row to seed, with defaults for the fields a test does not care about: harness `v1`,
/// 8/10 tests passed, $0.42, 120 s, a $5 cap, Zeroshot run id `zs-<id>`, ended when it started.
struct Run<'a> {
    id: &'a str,
    label: &'a str,
    task: &'a str,
    seed: i64,
    status: &'a str,
    eval_status: Option<&'a str>,
    score: Option<f64>,
    cost_usd: f64,
    wall_s: f64,
    started_at: f64,
    zeroshot_run_id: Option<String>,
    batch_id: Option<&'a str>,
    sweep_id: Option<&'a str>,
    model_override: Option<&'a str>,
    replay: Option<(&'a str, &'a str, Option<&'a str>)>,
}

impl<'a> Run<'a> {
    fn new(id: &'a str, label: &'a str, task: &'a str, seed: i64, status: &'a str) -> Self {
        Self {
            id,
            label,
            task,
            seed,
            status,
            eval_status: None,
            score: None,
            cost_usd: 0.42,
            wall_s: 120.0,
            started_at: 1000.0,
            zeroshot_run_id: Some(format!("zs-{id}")),
            batch_id: None,
            sweep_id: None,
            model_override: None,
            replay: None,
        }
    }
    fn eval(mut self, status: &'a str, score: Option<f64>) -> Self {
        self.eval_status = Some(status);
        self.score = score;
        self
    }
    /// Evaluated (`eval_status = 'ok'`) with this score.
    fn scored(self, score: f64) -> Self {
        self.eval("ok", Some(score))
    }
    fn cost(mut self, usd: f64) -> Self {
        self.cost_usd = usd;
        self
    }
    fn wall(mut self, seconds: f64) -> Self {
        self.wall_s = seconds;
        self
    }
    fn at(mut self, started_at: f64) -> Self {
        self.started_at = started_at;
        self
    }
    fn zeroshot_id(mut self, id: &str) -> Self {
        self.zeroshot_run_id = Some(id.to_owned());
        self
    }
    /// pb has not recorded the run's Zeroshot run id yet.
    fn no_zeroshot_id(mut self) -> Self {
        self.zeroshot_run_id = None;
        self
    }
    fn in_batch(mut self, batch_id: &'a str) -> Self {
        self.batch_id = Some(batch_id);
        self
    }
    fn in_sweep(mut self, sweep_id: &'a str) -> Self {
        self.sweep_id = Some(sweep_id);
        self
    }
    fn model(mut self, model: &'a str) -> Self {
        self.model_override = Some(model);
        self
    }
    /// A replay of `source` from `node` (through `through`, else to the end).
    fn replay_of(mut self, source: &'a str, node: &'a str, through: Option<&'a str>) -> Self {
        self.replay = Some((source, node, through));
        self
    }

    /// Inserts the row. The optional columns (`batch_id`, `sweep_id`, the replay ones) are
    /// written only when set, so a database without them takes the row too.
    fn insert(&self, conn: &Connection) {
        let limits = r#"{"max_cost_usd": 5.0}"#;
        let mut columns: Vec<(&str, &dyn ToSql)> = vec![
            ("run_id", &self.id),
            ("label", &self.label),
            ("version_id", &"v1"),
            ("task_id", &self.task),
            ("seed", &self.seed),
            ("status", &self.status),
            ("eval_status", &self.eval_status),
            ("score", &self.score),
            ("tests_passed", &8),
            ("tests_total", &10),
            ("cost_usd", &self.cost_usd),
            ("started_at", &self.started_at),
            ("ended_at", &self.started_at),
            ("wall_s", &self.wall_s),
            ("limits_json", &limits),
            ("zeroshot_run_id", &self.zeroshot_run_id),
        ];
        if let Some(batch_id) = &self.batch_id {
            columns.push(("batch_id", batch_id));
        }
        if let Some(sweep_id) = &self.sweep_id {
            columns.push(("sweep_id", sweep_id));
        }
        if let Some(model) = &self.model_override {
            columns.push(("model_override", model));
        }
        if let Some((source, node, through)) = &self.replay {
            columns.extend([
                ("replay_of", source as &dyn ToSql),
                ("replay_node", node),
                ("replay_through", through),
            ]);
        }
        insert_row(conn, "runs", &columns);
    }
}

/// A `batches` row to seed: by default `done`, harness `v1`, started at 1000 s, no tasks, no
/// seeds, no runner pid.
struct Batch<'a> {
    id: &'a str,
    label: &'a str,
    task_ids: String,
    seeds: String,
    status: &'a str,
    limits_json: Option<String>,
    parallel: Option<i64>,
    pid: Option<i64>,
    host: Option<String>,
}

impl<'a> Batch<'a> {
    fn new(id: &'a str, label: &'a str) -> Self {
        Self {
            id,
            label,
            task_ids: "[]".to_owned(),
            seeds: "[]".to_owned(),
            status: "done",
            limits_json: None,
            parallel: None,
            pid: None,
            host: None,
        }
    }
    fn tasks(mut self, tasks: &[&str]) -> Self {
        self.task_ids = serde_json::to_string(tasks).unwrap();
        self
    }
    fn seeds(mut self, seeds: &[i64]) -> Self {
        self.seeds = serde_json::to_string(seeds).unwrap();
        self
    }
    fn status(mut self, status: &'a str) -> Self {
        self.status = status;
        self
    }
    /// `running`, its runner being process `pid`.
    fn running(mut self, pid: impl Into<i64>) -> Self {
        self.status = "running";
        self.pid = Some(pid.into());
        self
    }
    /// The runner's host is this machine.
    fn on_this_host(mut self) -> Self {
        self.host = crate::source::hostname();
        self
    }
    fn limits(mut self, limits: serde_json::Value) -> Self {
        self.limits_json = Some(limits.to_string());
        self
    }
    fn parallel(mut self, parallel: i64) -> Self {
        self.parallel = Some(parallel);
        self
    }

    fn insert(&self, conn: &Connection) {
        let mut columns: Vec<(&str, &dyn ToSql)> = vec![
            ("id", &self.id),
            ("label", &self.label),
            ("version_id", &"v1"),
            ("task_ids", &self.task_ids),
            ("seeds", &self.seeds),
            ("limits_json", &self.limits_json),
            ("started_at", &1000.0),
            ("status", &self.status),
            ("pid", &self.pid),
            ("host", &self.host),
        ];
        if let Some(parallel) = &self.parallel {
            columns.push(("parallel", parallel));
        }
        insert_row(conn, "batches", &columns);
    }
}

/// A row of `node_executions` (with `duration_s`: see `schema/node_durations.sql`).
fn node_exec(conn: &Connection, run_id: &str, node: &str, cost: f64, duration_s: f64) {
    let columns: [(&str, &dyn ToSql); 4] = [
        ("run_id", &run_id),
        ("node_name", &node),
        ("cost_usd", &cost),
        ("duration_s", &duration_s),
    ];
    insert_row(conn, "node_executions", &columns);
}

fn insert_row(conn: &Connection, table: &str, columns: &[(&str, &dyn ToSql)]) {
    let names: Vec<&str> = columns.iter().map(|(name, _)| *name).collect();
    let marks: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        names.join(", "),
        marks.join(", ")
    );
    let values = rusqlite::params_from_iter(columns.iter().map(|(_, value)| value));
    conn.execute(&sql, values).unwrap();
}

/// Writes `contents` to `root/relative`, creating its directories.
fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}
