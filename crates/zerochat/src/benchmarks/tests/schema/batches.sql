-- Batches: one row per `pb run`, and each run's batch.
ALTER TABLE runs ADD COLUMN batch_id TEXT;
CREATE TABLE batches (
    id TEXT PRIMARY KEY, label TEXT, version_id TEXT, task_ids TEXT NOT NULL, seeds TEXT NOT NULL,
    limits_json TEXT, parallel INTEGER NOT NULL DEFAULT 1, started_at REAL NOT NULL, ended_at REAL,
    status TEXT NOT NULL, pid INTEGER, host TEXT
);
