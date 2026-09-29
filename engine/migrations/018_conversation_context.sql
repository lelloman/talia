-- Legacy context remains available to jobs admitted before this migration.
CREATE TABLE IF NOT EXISTS telegram_history_requests(seq INTEGER PRIMARY KEY, request_id INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS telegram_working_context(
    chat INTEGER NOT NULL, user INTEGER NOT NULL, epoch INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0, cutoff INTEGER, pending TEXT,
    PRIMARY KEY(chat,user,epoch)
);
-- Immutable source/summary payloads; only membership and processing flags change.
-- Summary coverage forms a DAG of earlier entry IDs, preserving provenance without
-- repeatedly expanding every original message ID into model input.
CREATE TABLE IF NOT EXISTS telegram_context_entries(
    id TEXT PRIMARY KEY, chat INTEGER NOT NULL, user INTEGER NOT NULL,
    epoch INTEGER NOT NULL, request_id INTEGER NOT NULL, kind TEXT NOT NULL,
    body TEXT NOT NULL, covered TEXT NOT NULL DEFAULT '[]',
    active INTEGER NOT NULL DEFAULT 1, processed INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS telegram_context_active ON telegram_context_entries(chat,user,epoch,active,request_id);
PRAGMA user_version=18;
