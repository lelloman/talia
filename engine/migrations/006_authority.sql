CREATE TABLE IF NOT EXISTS agent_principals(id TEXT PRIMARY KEY, version INTEGER NOT NULL, enabled INTEGER NOT NULL, policy TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS agent_credentials(digest TEXT PRIMARY KEY, principal TEXT NOT NULL REFERENCES agent_principals(id));
CREATE TABLE IF NOT EXISTS agent_security(id INTEGER PRIMARY KEY CHECK(id=1), digest_key BLOB NOT NULL, ceiling TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS agent_audit(id INTEGER PRIMARY KEY AUTOINCREMENT, principal TEXT NOT NULL, operation TEXT NOT NULL, status TEXT NOT NULL, body TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS agent_audit_principal ON agent_audit(principal,id);
CREATE INDEX IF NOT EXISTS agent_audit_status ON agent_audit(status);
CREATE TABLE IF NOT EXISTS agent_requests(principal TEXT NOT NULL, request_id TEXT NOT NULL, signature TEXT NOT NULL, audit_id INTEGER NOT NULL REFERENCES agent_audit(id), PRIMARY KEY(principal,request_id));
PRAGMA user_version=6;
