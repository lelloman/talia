CREATE TABLE IF NOT EXISTS dashboard_assignments(client TEXT NOT NULL REFERENCES dashboard_clients(id), slot TEXT NOT NULL, owner TEXT, revision INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY(client,slot));
PRAGMA user_version=8;
