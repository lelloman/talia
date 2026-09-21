CREATE TABLE IF NOT EXISTS live_outcomes(audit INTEGER PRIMARY KEY REFERENCES agent_audit(id),body TEXT NOT NULL);
PRAGMA user_version=9;
