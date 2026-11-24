-- Sweeps: replays of one source run's segments over several models.
CREATE TABLE sweeps (
    sweep_id TEXT PRIMARY KEY, created_at REAL NOT NULL, source_run TEXT NOT NULL,
    spec_json TEXT NOT NULL, label TEXT
);
ALTER TABLE runs ADD COLUMN sweep_id TEXT;
