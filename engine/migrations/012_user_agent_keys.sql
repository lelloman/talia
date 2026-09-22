CREATE TABLE IF NOT EXISTS user_agent_keys(
 id TEXT PRIMARY KEY, digest TEXT NOT NULL UNIQUE,
 subject TEXT NOT NULL REFERENCES dashboard_users(subject),
 auth_session TEXT NOT NULL, created INTEGER NOT NULL, expires INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS user_agent_keys_subject ON user_agent_keys(subject);
PRAGMA user_version=12;
