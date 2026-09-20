CREATE TABLE IF NOT EXISTS authored_definitions(kind TEXT NOT NULL, id TEXT NOT NULL, revision INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY(kind,id));
CREATE TABLE IF NOT EXISTS dashboard_packages(id TEXT PRIMARY KEY, body TEXT NOT NULL);
INSERT OR IGNORE INTO metadata VALUES('catalog_revision',0);
CREATE TRIGGER IF NOT EXISTS catalog_definition_insert AFTER INSERT ON definitions BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
CREATE TRIGGER IF NOT EXISTS catalog_definition_update AFTER UPDATE ON definitions WHEN old.body != new.body BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
CREATE TRIGGER IF NOT EXISTS catalog_definition_delete AFTER DELETE ON definitions BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
CREATE TRIGGER IF NOT EXISTS catalog_instance_insert AFTER INSERT ON instances BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
CREATE TRIGGER IF NOT EXISTS catalog_instance_delete AFTER DELETE ON instances BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
CREATE TRIGGER IF NOT EXISTS catalog_instance_update AFTER UPDATE ON instances WHEN
 json_extract(old.body,'$.definition') IS NOT json_extract(new.body,'$.definition') OR
 json_extract(old.body,'$.params') IS NOT json_extract(new.body,'$.params') OR
 json_extract(old.body,'$.history_count') IS NOT json_extract(new.body,'$.history_count') OR
 json_extract(old.body,'$.history_age_ms') IS NOT json_extract(new.body,'$.history_age_ms')
 BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
CREATE TRIGGER IF NOT EXISTS catalog_monitor_insert AFTER INSERT ON monitoring_config BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
CREATE TRIGGER IF NOT EXISTS catalog_monitor_update AFTER UPDATE ON monitoring_config WHEN old.body != new.body BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
CREATE TRIGGER IF NOT EXISTS catalog_monitor_delete AFTER DELETE ON monitoring_config BEGIN UPDATE metadata SET value=value+1 WHERE key='catalog_revision'; END;
PRAGMA user_version=5;
