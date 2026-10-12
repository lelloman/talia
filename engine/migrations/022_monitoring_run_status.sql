-- The scheduler polls active runs frequently; completed history must not be scanned.
CREATE INDEX IF NOT EXISTS monitoring_runs_status ON monitoring_runs(status);
PRAGMA user_version=22;
