-- Replays: a run that re-runs a source run from one node (through another), maybe on another
-- model; node executions and step scores say which execution they belong to.
ALTER TABLE runs ADD COLUMN model_override TEXT;
ALTER TABLE runs ADD COLUMN replay_of TEXT;
ALTER TABLE runs ADD COLUMN replay_node TEXT;
ALTER TABLE runs ADD COLUMN replay_through TEXT;
ALTER TABLE node_executions ADD COLUMN execution INTEGER;
ALTER TABLE node_executions ADD COLUMN node_id TEXT;
ALTER TABLE node_executions ADD COLUMN model TEXT;
ALTER TABLE step_scores ADD COLUMN execution INTEGER;
