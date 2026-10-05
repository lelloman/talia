use super::*;
/// Shared "AI investigation sources": DataSource IDs approved for live probes by both
/// Telegram investigations and app chat. Stored with the Telegram settings, where
/// administrators already manage it.
pub(crate) fn approved_sources(s: &crate::store::Store) -> Result<Vec<String>> {
    Ok(s.telegram_config()?.sources)
}
pub fn definitions() -> Vec<Value> {
    vec![
        json!({"name":"monitoring_snapshot","description":"Catalogue of monitoring variables, alerts, DataSource IDs (probe_allowed marks sources monitoring_probe may query) and recent report runs. No writes. Each variable has a readable preview of its current value: long numeric series are summarized as {series, samples, min, max, mean, last} (nulls are missing samples) and long lists or strings are shortened. Use monitoring_read with the variable ID for exact full data. history_count and history_age_ms are retention limits, not actual sample counts. has_value, quality and timestamp describe the cached value. Empty alerts means no recorded alerts, not verified health. Reports are scheduled report runs, not conversation or investigation history.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"monitoring_read","description":"Read a cached variable, its retained history, or a report run by ID. Does not execute variable getters. history_count/history_age_ms configure retention, so an empty history can be valid. Internal state and exposed value are independent; undefined state does not invalidate a populated value. Check has_value, quality and timestamp when interpreting cached data.","inputSchema":{"type":"object","properties":{"kind":{"enum":["variable","history","report"]},"id":{"type":"string"}},"required":["kind","id"],"additionalProperties":false}}),
        json!({"name":"monitoring_probe","description":"Run a read-only SourceRequest against an administrator-approved DataSource. Prometheus query/range and HTTP GET/HEAD only. No arbitrary URL, pipeline execution or writes.","inputSchema":{"type":"object","properties":{"source":{"type":"string"},"request":{"oneOf":[
            {"type":"object","properties":{"kind":{"const":"query"},"query":{"type":"string"},"time":{"type":"number","description":"Unix seconds"}},"required":["kind","query"],"additionalProperties":false},
            {"type":"object","properties":{"kind":{"const":"range"},"query":{"type":"string"},"start":{"type":"number"},"end":{"type":"number"},"step":{"type":"number","description":"Times and step are seconds"}},"required":["kind","query","start","end","step"],"additionalProperties":false},
            {"type":"object","properties":{"kind":{"const":"http"},"path":{"type":"string","description":"Path within the configured source origin"},"method":{"enum":["GET","HEAD"]},"text":{"type":"boolean"}},"required":["kind"],"additionalProperties":false}
        ]}},"required":["source","request"],"additionalProperties":false}}),
    ].into_iter().map(|v|json!({"type":"function","function":{"name":v["name"],"description":v["description"],"parameters":v["inputSchema"]}})).collect()
}
const SNAPSHOT_LIMIT: usize = 30 * 1024;
pub async fn execute(engine: &Engine, name: &str, args: Value) -> Result<Value> {
    let result = match name {
        "monitoring_snapshot" => {
            let s = engine.store.borrow();
            let approved = approved_sources(&s)?;
            // A catalogue with readable previews; full values stay with monitoring_read.
            let mut values: Vec<Value> = s
                .instances()?
                .into_iter()
                .map(|i| json!({"id":i.id,"definition":i.definition,"has_value":i.has_value,"quality":i.quality,
                    "timestamp":i.timestamp,"revision":i.revision,"history_count":i.history_count,
                    "history_age_ms":i.history_age_ms,"value":crate::value::preview(&i.value)}))
                .collect();
            let sources: Vec<_> = s
                .monitoring_config()?
                .sources
                .into_iter()
                .map(|v| json!({"id":v.id,"kind":v.kind,"probe_allowed":approved.contains(&v.id)}))
                .collect();
            let mut q=s.conn.prepare("SELECT id,report,status,created FROM report_runs ORDER BY created DESC LIMIT 30").map_err(err)?;
            let reports=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"report":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"created":r.get::<_,i64>(3)?}))).map_err(err)?.collect::<std::result::Result<Vec<_>,_>>().map_err(err)?;
            let all_alerts = s.alerts()?;
            let alerts: Vec<_> = all_alerts.iter().take(40).collect();
            let mut snapshot = json!({"now":engine.now(),"values":values,"sources":sources,"alerts":alerts,
                "alerts_total":all_alerts.len(),"reports":reports});
            // Always fit the tool limit: drop the largest previews first and say so.
            while serde_json::to_vec(&snapshot).map_err(err)?.len() > SNAPSHOT_LIMIT {
                let Some((index, _)) = values.iter().enumerate()
                    .filter(|(_, v)| !v["value"].is_string())
                    .max_by_key(|(_, v)| serde_json::to_vec(&v["value"]).map(|b| b.len()).unwrap_or(0))
                else { return Err("monitoring snapshot too large; read resources individually".into()) };
                values[index]["value"] = json!("omitted for size; use monitoring_read");
                snapshot["values"] = json!(values);
            }
            snapshot
        }
        "monitoring_read" => {
            let s = engine.store.borrow();
            let id = args["id"].as_str().ok_or("id required")?;
            match args["kind"].as_str() {
                Some("variable") => json!(s.instance(id)?),
                Some("history") => json!(s.history(id, engine.now())?),
                Some("report") => {
                    let r = s.report_run(id)?;
                    json!({"id":r.id,"status":r.status,"content":r.content,"error":r.error})
                }
                _ => return Err("invalid read kind".into()),
            }
        }
        "monitoring_probe" => {
            let id = args["source"].as_str().ok_or("source required")?;
            let source = {
                let s = engine.store.borrow();
                if !approved_sources(&s)?.iter().any(|v| v == id) {
                    return Err("probe source not approved".into());
                }
                s.monitoring_config()?.source(id)?.clone()
            };
            let request: crate::sources::SourceRequest =
                serde_json::from_value(args["request"].clone())
                    .map_err(|_| "invalid source request")?;
            if request.has_effect() {
                return Err("Probes cannot mutate sources".into());
            }
            let value = crate::sources::Adapters::new()?
                .fetch(&source, &request)
                .await?;
            // Revalidate revocation and source permission before releasing delayed data.
            if !approved_sources(&engine.store.borrow())?.iter().any(|v| v == id) {
                return Err("probe source permission revoked".into());
            }
            let current = engine
                .store
                .borrow()
                .monitoring_config()?
                .source(id)?
                .clone();
            if serde_json::to_value(&current).map_err(err)?
                != serde_json::to_value(&source).map_err(err)?
            {
                return Err("probe source changed".into());
            }
            value
        }
        _ => return Err("AI tool forbidden".into()),
    };
    if serde_json::to_vec(&result).map_err(err)?.len() > 32768 {
        return Err("Tool response limit; query individual resources".into());
    }
    Ok(result)
}

pub fn allowed(name: &str) -> bool {
    matches!(
        name,
        "monitoring_snapshot" | "monitoring_read" | "monitoring_probe"
    )
}
