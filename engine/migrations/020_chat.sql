-- Server-owned app chat sessions (web and native). Admin-only in v1.
CREATE TABLE IF NOT EXISTS chat_sessions(
    id TEXT PRIMARY KEY, subject TEXT NOT NULL, client_id TEXT NOT NULL, title TEXT NOT NULL,
    report TEXT, created INTEGER NOT NULL, updated INTEGER NOT NULL, deleted INTEGER NOT NULL DEFAULT 0,
    UNIQUE(subject, client_id)
);
CREATE INDEX IF NOT EXISTS chat_sessions_owner ON chat_sessions(subject, deleted, updated);
-- Request IDs are global, so conversation-core entry IDs never collide across sessions.
-- client_id makes a retried send idempotent within its session.
CREATE TABLE IF NOT EXISTS chat_requests(
    id INTEGER PRIMARY KEY AUTOINCREMENT, session TEXT NOT NULL REFERENCES chat_sessions(id),
    client_id TEXT NOT NULL, status TEXT NOT NULL, body TEXT NOT NULL,
    answer TEXT, error TEXT, created INTEGER NOT NULL, finished INTEGER,
    UNIQUE(session, client_id)
);
CREATE INDEX IF NOT EXISTS chat_requests_pending ON chat_requests(status, session, id);
-- Conversation-core tables (see engine/src/conversation.rs), keyed by session.
CREATE TABLE IF NOT EXISTS chat_history(seq INTEGER PRIMARY KEY AUTOINCREMENT, session TEXT NOT NULL, role TEXT NOT NULL, body TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS chat_history_requests(seq INTEGER PRIMARY KEY, request_id INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS chat_working_context(
    session TEXT PRIMARY KEY, revision INTEGER NOT NULL DEFAULT 0, cutoff INTEGER, pending TEXT
);
CREATE TABLE IF NOT EXISTS chat_context_entries(
    id TEXT PRIMARY KEY, session TEXT NOT NULL, request_id INTEGER NOT NULL, kind TEXT NOT NULL,
    body TEXT NOT NULL, covered TEXT NOT NULL DEFAULT '[]',
    active INTEGER NOT NULL DEFAULT 1, processed INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS chat_context_active ON chat_context_entries(session, active, request_id);
PRAGMA user_version=20;
