CREATE TABLE IF NOT EXISTS dashboard_clients(id TEXT PRIMARY KEY, credential TEXT NOT NULL UNIQUE, name TEXT NOT NULL, platform TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS dashboard_slots(client TEXT NOT NULL REFERENCES dashboard_clients(id), id TEXT NOT NULL, owner TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(client,id));
CREATE TABLE IF NOT EXISTS dashboard_instances(id TEXT PRIMARY KEY, client TEXT NOT NULL, slot TEXT NOT NULL);
PRAGMA user_version=7;
