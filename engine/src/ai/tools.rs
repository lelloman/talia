use super::*;
pub fn definitions() -> Vec<Value> {
    vec![
        json!({"name":"monitoring_snapshot","description":"Read monitoring values and alerts, source IDs and available reports. No writes. Values retain tagged wire encoding. history_count and history_age_ms are retention limits, not actual sample counts or guarantees of history; old samples may have expired. state is private computation/cache state, independent of exposed value: undefined state with a populated value is valid. has_value, quality and timestamp describe the cached value. Empty alerts means no recorded alerts, not verified health. Reports are scheduled report runs, not conversation or investigation history.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"monitoring_read","description":"Read a cached variable, its retained history, or a report run by ID. Does not execute variable getters. history_count/history_age_ms configure retention, so an empty history can be valid. Internal state and exposed value are independent; undefined state does not invalidate a populated value. Check has_value, quality and timestamp when interpreting cached data.","inputSchema":{"type":"object","properties":{"kind":{"enum":["variable","history","report"]},"id":{"type":"string"}},"required":["kind","id"],"additionalProperties":false}}),
        json!({"name":"monitoring_probe","description":"Run a read-only SourceRequest against an administrator-approved DataSource. Prometheus query/range and HTTP GET/HEAD only. No arbitrary URL, pipeline execution or writes.","inputSchema":{"type":"object","properties":{"source":{"type":"string"},"request":{"oneOf":[
            {"type":"object","properties":{"kind":{"const":"query"},"query":{"type":"string"},"time":{"type":"number","description":"Unix seconds"}},"required":["kind","query"],"additionalProperties":false},
            {"type":"object","properties":{"kind":{"const":"range"},"query":{"type":"string"},"start":{"type":"number"},"end":{"type":"number"},"step":{"type":"number","description":"Times and step are seconds"}},"required":["kind","query","start","end","step"],"additionalProperties":false},
            {"type":"object","properties":{"kind":{"const":"http"},"path":{"type":"string","description":"Path within the configured source origin"},"method":{"enum":["GET","HEAD"]},"text":{"type":"boolean"}},"required":["kind"],"additionalProperties":false}
        ]}},"required":["source","request"],"additionalProperties":false}}),
    ].into_iter().map(|v|json!({"type":"function","function":{"name":v["name"],"description":v["description"],"parameters":v["inputSchema"]}})).collect()
}
pub async fn execute(engine: &Engine, name: &str, args: Value) -> Result<Value> {
    let result = match name {
        "monitoring_snapshot" => {
            let s = engine.store.borrow();
            let c = s.telegram_config()?;
            let values = s.instances()?;
            let sources: Vec<_> = s
                .monitoring_config()?
                .sources
                .into_iter()
                .map(|v| json!({"id":v.id,"kind":v.kind,"probe_allowed":c.sources.contains(&v.id)}))
                .collect();
            let mut q=s.conn.prepare("SELECT id,report,status,created FROM report_runs ORDER BY created DESC LIMIT 30").map_err(err)?;
            let reports=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"report":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"created":r.get::<_,i64>(3)?}))).map_err(err)?.collect::<std::result::Result<Vec<_>,_>>().map_err(err)?;
            json!({"now":engine.now(),"values":values,"sources":sources,"alerts":s.alerts()?,"reports":reports})
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
                if !s.telegram_config()?.sources.iter().any(|v| v == id) {
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
            if !engine
                .store
                .borrow()
                .telegram_config()?
                .sources
                .iter()
                .any(|v| v == id)
            {
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
