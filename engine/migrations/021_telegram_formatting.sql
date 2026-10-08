ALTER TABLE telegram_outbox ADD COLUMN entities TEXT NOT NULL DEFAULT '[]';
PRAGMA user_version=21;
