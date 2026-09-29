CREATE TABLE IF NOT EXISTS store_push_subscriptions(id TEXT PRIMARY KEY, owner TEXT NOT NULL, subject TEXT NOT NULL, session TEXT NOT NULL, application TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS store_push_outbox(id TEXT PRIMARY KEY, subscription_id TEXT NOT NULL REFERENCES store_push_subscriptions(id) ON DELETE CASCADE, body TEXT NOT NULL, at INTEGER NOT NULL, status TEXT NOT NULL DEFAULT 'pending');
CREATE INDEX IF NOT EXISTS store_push_pending ON store_push_outbox(status,at);
CREATE TRIGGER IF NOT EXISTS store_push_capacity BEFORE INSERT ON store_push_outbox WHEN (SELECT count(*) FROM store_push_outbox WHERE status='pending')>=100000 BEGIN SELECT RAISE(ABORT,'notification outbox capacity'); END;
CREATE TRIGGER IF NOT EXISTS store_push_report_insert AFTER INSERT ON report_runs WHEN json_extract(NEW.body,'$.send')=1 AND NEW.status IN ('completed','partial','failed') BEGIN
INSERT OR IGNORE INTO store_push_outbox(id,subscription_id,body,at)
SELECT 'report:'||NEW.id||':'||s.id,s.id,json_object(
'event_id','report:'||NEW.id||':'||s.id,'application',s.application,'type','report.completed',
'target',json_object('subject',s.subject,'subscription_id',s.id),'level',coalesce(json_extract(NEW.body,'$.content.severity'),'unknown'),
'tags',json_array('report'),'occurred_at',unixepoch(),'replacement_key',NULL,'occurrence',0,'revision',0,
'payload',json_object('run_id',NEW.id,'report_id',json_extract(NEW.body,'$.definition.id'),'status',NEW.status,
'severity',coalesce(json_extract(NEW.body,'$.content.severity'),'unknown'),'title',substr(coalesce(json_extract(NEW.body,'$.content.subject'),'Report finished'),1,160),
'summary',substr(coalesce(json_extract(NEW.body,'$.content.summary'),json_extract(NEW.body,'$.error'),''),1,2000))),unixepoch()
FROM store_push_subscriptions s; END;
CREATE TRIGGER IF NOT EXISTS store_push_incident_insert AFTER INSERT ON alert_entities WHEN NEW.kind='alert' BEGIN
INSERT OR IGNORE INTO store_push_outbox(id,subscription_id,body,at)
SELECT 'incident:'||NEW.id||':'||json_extract(NEW.body,'$.occurrence')||':'||json_extract(NEW.body,'$.revision')||':'||s.id,s.id,json_object(
'event_id','incident:'||NEW.id||':'||json_extract(NEW.body,'$.occurrence')||':'||json_extract(NEW.body,'$.revision')||':'||s.id,
'application',s.application,'type','incident.state','target',json_object('subject',s.subject,'subscription_id',s.id),
'level',CASE WHEN json_extract(NEW.body,'$.active')=0 THEN 'info' WHEN json_extract(NEW.body,'$.severity') IN ('warning','critical','error','info') THEN json_extract(NEW.body,'$.severity') ELSE 'warning' END,
'tags',json_array('incident'),'occurred_at',unixepoch(),'replacement_key',NEW.id,
'occurrence',json_extract(NEW.body,'$.occurrence'),'revision',json_extract(NEW.body,'$.revision'),
'payload',json_object('key',NEW.id,'active',json_extract(NEW.body,'$.active'),'stage',json_extract(NEW.body,'$.stage'),
'acknowledged',json_extract(NEW.body,'$.acknowledgement') IS NOT NULL,'title',substr(NEW.id,1,160),'summary',substr(json_extract(NEW.body,'$.message'),1,2000))),unixepoch()
FROM store_push_subscriptions s; END;
CREATE TRIGGER IF NOT EXISTS store_push_report_update AFTER UPDATE ON report_runs WHEN json_extract(NEW.body,'$.send')=1 AND NEW.status IN ('completed','partial','failed') AND OLD.status NOT IN ('completed','partial','failed') BEGIN
INSERT OR IGNORE INTO store_push_outbox(id,subscription_id,body,at)
SELECT 'report:'||NEW.id||':'||s.id,s.id,json_object(
'event_id','report:'||NEW.id||':'||s.id,'application',s.application,'type','report.completed',
'target',json_object('subject',s.subject,'subscription_id',s.id),'level',coalesce(json_extract(NEW.body,'$.content.severity'),'unknown'),
'tags',json_array('report'),'occurred_at',unixepoch(),'replacement_key',NULL,'occurrence',0,'revision',0,
'payload',json_object('run_id',NEW.id,'report_id',json_extract(NEW.body,'$.definition.id'),'status',NEW.status,
'severity',coalesce(json_extract(NEW.body,'$.content.severity'),'unknown'),'title',substr(coalesce(json_extract(NEW.body,'$.content.subject'),'Report finished'),1,160),
'summary',substr(coalesce(json_extract(NEW.body,'$.content.summary'),json_extract(NEW.body,'$.error'),''),1,2000))),unixepoch()
FROM store_push_subscriptions s; END;
CREATE TRIGGER IF NOT EXISTS store_push_incident_update AFTER UPDATE ON alert_entities WHEN NEW.kind='alert' AND (json_extract(NEW.body,'$.occurrence')<>json_extract(OLD.body,'$.occurrence') OR json_extract(NEW.body,'$.revision')<>json_extract(OLD.body,'$.revision')) BEGIN
INSERT OR IGNORE INTO store_push_outbox(id,subscription_id,body,at)
SELECT 'incident:'||NEW.id||':'||json_extract(NEW.body,'$.occurrence')||':'||json_extract(NEW.body,'$.revision')||':'||s.id,s.id,json_object(
'event_id','incident:'||NEW.id||':'||json_extract(NEW.body,'$.occurrence')||':'||json_extract(NEW.body,'$.revision')||':'||s.id,
'application',s.application,'type','incident.state','target',json_object('subject',s.subject,'subscription_id',s.id),
'level',CASE WHEN json_extract(NEW.body,'$.active')=0 THEN 'info' WHEN json_extract(NEW.body,'$.severity') IN ('warning','critical','error','info') THEN json_extract(NEW.body,'$.severity') ELSE 'warning' END,
'tags',json_array('incident'),'occurred_at',unixepoch(),'replacement_key',NEW.id,
'occurrence',json_extract(NEW.body,'$.occurrence'),'revision',json_extract(NEW.body,'$.revision'),
'payload',json_object('key',NEW.id,'active',json_extract(NEW.body,'$.active'),'stage',json_extract(NEW.body,'$.stage'),
'acknowledged',json_extract(NEW.body,'$.acknowledgement') IS NOT NULL,'title',substr(NEW.id,1,160),'summary',substr(json_extract(NEW.body,'$.message'),1,2000))),unixepoch()
FROM store_push_subscriptions s; END;
PRAGMA user_version=19;
