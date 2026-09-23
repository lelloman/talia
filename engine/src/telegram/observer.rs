use super::*;
use crate::reports::agent::{self, Provider};
pub async fn profiles() -> Result<Vec<String>> {
    let path = std::env::var("TALIA_TELEGRAM_OBSERVERS")
        .map_err(|_| "TALIA_TELEGRAM_OBSERVERS is not configured")?;
    let map: std::collections::BTreeMap<String, Provider> =
        serde_json::from_slice(&agent::read_file(&path, 65536).await?)
            .map_err(|_| "invalid observer configuration")?;
    Ok(map.into_keys().collect())
}
pub async fn provider(name: &str) -> Result<Provider> {
    let path = provider_path()?;
    let p = agent::config(&path, name).await?;
    use simple_agents_client::protocol::Capability;
    if p.capabilities.iter().any(|c| {
        !matches!(
            c,
            Capability::Network | Capability::RepositoryRead | Capability::SessionResume
        )
    }) {
        return Err("observer profile has non-observational capabilities".into());
    }
    Ok(p)
}
fn provider_path() -> Result<String> {
    #[cfg(test)]
    if let Some(path) = TEST_PROVIDERS.with(|v| v.borrow().clone()) {
        return Ok(path);
    }
    std::env::var("TALIA_TELEGRAM_OBSERVERS")
        .map_err(|_| "TALIA_TELEGRAM_OBSERVERS is not configured".into())
}
#[cfg(test)]
thread_local! {pub static TEST_PROVIDERS:std::cell::RefCell<Option<String>>=const {std::cell::RefCell::new(None)};}
impl Store {
    pub fn telegram_observer_auth(&self, token: &str) -> Result<Value> {
        if !token.starts_with("to_") || token.len() != 67 {
            return Err("unauthorized".into());
        }
        let fingerprint = transport::hash(token);
        let valid: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM telegram_observer WHERE id=1 AND hash=?)",
                [&fingerprint],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !valid {
            return Err("unauthorized".into());
        }
        Ok(json!({"observer":true,"admin":false,"id":fingerprint}))
    }
}
pub fn tools() -> Vec<Value> {
    vec![
        json!({"name":"observer_snapshot","description":"Read monitoring values and alerts, source IDs and available reports. No writes. Values retain tagged wire encoding.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"observer_read","description":"Read a cached variable, its retained history, or a report run by ID. Does not execute variable getters.","inputSchema":{"type":"object","properties":{"kind":{"enum":["variable","history","report"]},"id":{"type":"string"}},"required":["kind","id"],"additionalProperties":false}}),
        json!({"name":"observer_probe","description":"Run a read-only SourceRequest against an administrator-approved DataSource. Prometheus query/range and HTTP GET/HEAD only. No arbitrary URL, pipeline execution or writes.","inputSchema":{"type":"object","properties":{"source":{"type":"string"},"request":{"type":"object"}},"required":["source","request"],"additionalProperties":false}}),
    ]
}
pub async fn execute(engine: &Engine, token: &str, name: &str, args: Value) -> Result<Value> {
    engine.store.borrow().telegram_observer_auth(token)?;
    let result = match name {
        "observer_snapshot" => {
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
            json!({"values":values,"sources":sources,"alerts":s.alerts()?,"reports":reports})
        }
        "observer_read" => {
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
        "observer_probe" => {
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
                return Err("observer cannot mutate sources".into());
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
            value
        }
        _ => return Err("observer tool forbidden".into()),
    };
    engine.store.borrow().telegram_observer_auth(token)?;
    if serde_json::to_vec(&result).map_err(err)?.len() > 262144 {
        return Err("observer response limit; query individual resources".into());
    }
    Ok(result)
}
