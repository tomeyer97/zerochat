-- pb.sqlite as an older pb (before batches) writes it: only the tables and columns zerochat
-- reads. Each other file in this directory is a later pb's addition, applied on top.
CREATE TABLE harness_versions (
    version_id TEXT PRIMARY KEY, zeroshot_version TEXT, git_commit TEXT, runtime_json TEXT
);
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY, label TEXT, version_id TEXT, task_id TEXT NOT NULL, seed INTEGER NOT NULL,
    status TEXT NOT NULL, exit_reason TEXT, harness_outcome TEXT, zeroshot_run_id TEXT,
    started_at REAL NOT NULL, ended_at REAL, wall_s REAL, cost_usd REAL, reported_cost_usd REAL,
    score REAL, tests_passed INTEGER, tests_total INTEGER, eval_status TEXT, eval_error TEXT,
    regression_passed INTEGER, regression_total INTEGER, limits_json TEXT, artifact_dir TEXT
);
CREATE TABLE test_results (run_id TEXT, branch TEXT, name TEXT, status TEXT);
CREATE TABLE step_scores (
    run_id TEXT, seq INTEGER, node_id TEXT, score REAL, tests_passed INTEGER, tests_total INTEGER
);
CREATE TABLE node_executions (run_id TEXT, node_name TEXT, cost_usd REAL);
