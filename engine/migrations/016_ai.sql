CREATE TABLE IF NOT EXISTS ai_runs(id TEXT PRIMARY KEY,status TEXT NOT NULL,created INTEGER NOT NULL,body TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS ai_runs_status ON ai_runs(status);
PRAGMA user_version=16;
