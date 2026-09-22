CREATE TABLE IF NOT EXISTS report_definitions(id TEXT PRIMARY KEY, body TEXT NOT NULL, next_due INTEGER);
CREATE TABLE IF NOT EXISTS report_runs(id TEXT PRIMARY KEY, report TEXT NOT NULL, status TEXT NOT NULL, created INTEGER NOT NULL, body TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS report_runs_status ON report_runs(status,created);
CREATE INDEX IF NOT EXISTS report_runs_report ON report_runs(report,created);
CREATE TABLE IF NOT EXISTS report_requests(id TEXT PRIMARY KEY, signature TEXT NOT NULL, result TEXT NOT NULL);
PRAGMA user_version=13;
