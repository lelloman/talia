-- Retain the original step document as evidence, but remove its execution capability.
UPDATE report_definitions
SET body=json_set(body,
    '$.steps',json((SELECT json_group_array(json(CASE WHEN json_extract(value,'$.kind')='simple_agents'
        THEN json_object('id',json_extract(value,'$.id'),'optional',json(CASE WHEN json_extract(value,'$.optional') THEN 'true' ELSE 'false' END),
            'kind','unavailable','reason','Simple Agents integration was removed; replace this step','original',json(value))
        ELSE value END)) FROM json_each(report_definitions.body,'$.steps'))),
    '$.enabled',json('false'),'$.version',json_extract(body,'$.version')+1),next_due=NULL
WHERE EXISTS(SELECT 1 FROM json_each(body,'$.steps') WHERE json_extract(value,'$.kind')='simple_agents');

UPDATE report_runs
SET body=json_set(body,'$.definition.steps',json((SELECT json_group_array(json(CASE WHEN json_extract(value,'$.kind')='simple_agents'
        THEN json_object('id',json_extract(value,'$.id'),'optional',json(CASE WHEN json_extract(value,'$.optional') THEN 'true' ELSE 'false' END),
            'kind','unavailable','reason','Simple Agents integration was removed; replace this step','original',json(value))
        ELSE value END)) FROM json_each(report_runs.body,'$.definition.steps'))))
WHERE EXISTS(SELECT 1 FROM json_each(body,'$.definition.steps') WHERE json_extract(value,'$.kind')='simple_agents');
UPDATE report_runs SET body=json_remove(json_set(body,'$.retired_execution',json_extract(body,'$.agent')),'$.agent')
WHERE json_type(body,'$.agent') IS NOT NULL;
UPDATE report_runs SET status='failed',body=json_set(body,'$.status','failed','$.error','Simple Agents integration was removed; execution stopped')
WHERE status IN ('queued','running') AND EXISTS(SELECT 1 FROM json_each(body,'$.definition.steps') WHERE json_extract(value,'$.kind')='unavailable');

UPDATE telegram_config SET body=json_remove(body,'$.observer','$.sources');
UPDATE telegram_jobs SET status='cancelled',body=json_set(body,'$.error','Simple Agents integration was removed; investigation stopped')
WHERE status IN ('queued','running');
UPDATE telegram_outbox SET status='failed' WHERE kind='chat' AND status='pending';
-- Unlike historical run evidence, obsolete credentials must not survive the upgrade.
DROP TABLE IF EXISTS telegram_observer;
PRAGMA user_version=15;
