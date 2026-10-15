-- SQLite indexes append rowid, so instance-only indexing serves newest-run reads without sorting history.
-- Deployment /tmp is a bounded tmpfs; index sorting must not spill there.
PRAGMA temp_store=MEMORY;
CREATE INDEX IF NOT EXISTS monitoring_runs_latest ON monitoring_runs(instance);
PRAGMA temp_store=DEFAULT;
PRAGMA user_version=23;
