CREATE TABLE IF NOT EXISTS dashboard_users(subject TEXT PRIMARY KEY, name TEXT NOT NULL, admin INTEGER NOT NULL DEFAULT 0, default_dashboard TEXT);
CREATE TABLE IF NOT EXISTS dashboard_access(dashboard TEXT PRIMARY KEY, owner TEXT NOT NULL, public INTEGER NOT NULL DEFAULT 0, viewers TEXT NOT NULL DEFAULT '[]', revision INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS dashboard_access_audit(seq INTEGER PRIMARY KEY AUTOINCREMENT, actor TEXT NOT NULL, request TEXT NOT NULL, body TEXT NOT NULL, outcome TEXT NOT NULL, UNIQUE(actor,request));
PRAGMA user_version=11;
CREATE TRIGGER IF NOT EXISTS dashboard_access_delete AFTER DELETE ON dashboard_packages BEGIN
 DELETE FROM dashboard_access WHERE dashboard=old.id;
 UPDATE dashboard_users SET default_dashboard=NULL WHERE default_dashboard=old.id;
END;
