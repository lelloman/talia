//! Read-only native Overview projection. Global service/report data is admin-only.
use crate::{
    reports::Run,
    store::{Result, Store},
};
use serde_json::{json, Value};
impl Store {
    pub fn native_overview(&mut self, subject: &str, name: &str, now: i64) -> Result<Value> {
        self.user_seen(subject, name)
            .map_err(|_| "account unavailable")?;
        if !self
            .user_admin(subject)
            .map_err(|_| "account unavailable")?
        {
            return Err("forbidden".into());
        }
        let sample = self
            .instances()?
            .into_iter()
            .find(|i| i.id == "homelab-summary");
        let mut services = vec![];
        let mut sampled_at = None;
        let mut quality_good = false;
        if let Some(sample) = sample.filter(|i| i.has_value) {
            sampled_at = Some(sample.timestamp);
            quality_good = sample.quality == "good";
            let data = decode(&sample.value["value"]);
            if let Some(targets) = data["targets"].as_array() {
                for target in targets.iter().take(200) {
                    let state = target["status"].as_str().unwrap_or("unknown");
                    services.push(json!({"name":target["name"].as_str().unwrap_or("Service"),"status":if ["up","down"].contains(&state){state}else{"unknown"}}));
                }
            }
        }
        let mut statement = self
            .conn
            .prepare("SELECT body FROM report_runs ORDER BY created DESC,id DESC LIMIT 20")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut reports = vec![];
        for row in rows {
            let run: Run = serde_json::from_str(&row.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            reports.push(json!({"id":run.id,"report":run.definition.id,"status":run.status,"created":run.created,
                "title":run.content.as_ref().map(|c|c.subject.chars().take(256).collect::<String>()),
                "summary":run.content.as_ref().map(|c|c.summary.chars().take(4000).collect::<String>())}));
        }
        Ok(
            json!({"name":name,"fetchedAt":now,"sampledAt":sampled_at,"stale":!quality_good || sampled_at.is_none_or(|t|now.saturating_sub(t)>120_000),"services":services,"reports":reports}),
        )
    }
}
fn decode(v: &Value) -> Value {
    match v[0].as_str() {
        Some("object") => Value::Object(
            v[1].as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| Some((p[0].as_str()?.to_owned(), decode(&p[1]))))
                .collect(),
        ),
        Some("array") => Value::Array(v[1].as_array().into_iter().flatten().map(decode).collect()),
        Some("string" | "boolean" | "number") => v[1].clone(),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overview_denies_viewers_and_distinguishes_missing_samples() {
        let mut store = Store::open(":memory:").unwrap();
        assert_eq!(
            store
                .native_overview("issuer#viewer", "Viewer", 1000)
                .unwrap_err(),
            "forbidden"
        );
        store.user_bootstrap("issuer#admin").unwrap();
        let overview = store
            .native_overview("issuer#admin", "Admin", 1000)
            .unwrap();
        assert_eq!(overview["stale"], true);
        assert_eq!(overview["sampledAt"], Value::Null);
        assert_eq!(overview["services"], json!([]));
        assert_eq!(overview["reports"], json!([]));
        assert_eq!(overview["name"], "Admin");
    }
    #[test]
    fn overview_projects_live_samples_and_report_summaries_without_private_fields() {
        let mut store = Store::open(":memory:").unwrap();
        store.user_bootstrap("issuer#admin").unwrap();
        let definition:crate::store::Definition=serde_json::from_value(json!({"id":"snapshot","version":1,"source":"","kind":"stored","value_schema":"any","state_schema":"any","dependencies":[]})).unwrap();
        store.create_definition(&definition).unwrap();
        let instance:crate::store::Instance=serde_json::from_value(json!({"id":"homelab-summary","definition":"snapshot","params":crate::value::undefined(),"state":crate::value::undefined(),"value":crate::value::from_json(&json!({"targets":[{"name":"Store","status":"up","instance":"private.internal:9091"},{"name":"Music","status":"down"}]})),"has_value":true,"timestamp":1000,"quality":"good","revision":1,"generation":1,"history_count":0,"history_age_ms":0})).unwrap();
        store.create_instance(&instance).unwrap();
        let definition:crate::reports::Definition=serde_json::from_value(json!({"id":"morning","version":1,"enabled":false,"schedule":null,"steps":[],"compose":"secret-source","destinations":[]})).unwrap();
        for (id, created) in [("older", 10), ("newer", 20)] {
            let run:Run=serde_json::from_value(json!({"id":id,"definition":definition,"actor":"private-actor","created":created,"deadline":100,"period_start":0,"status":"complete","send":false,"index":0,"outputs":{},"content":{"subject":"Morning","summary":"Service warning","sections":[]},"html":null,"text":null,"deliveries":[],"error":null})).unwrap();
            store.report_put(&run).unwrap();
        }
        let value = store
            .native_overview("issuer#admin", "Admin", 2000)
            .unwrap();
        assert_eq!(
            value["services"],
            json!([{"name":"Store","status":"up"},{"name":"Music","status":"down"}])
        );
        assert_eq!(value["stale"], false);
        assert_eq!(value["reports"][0]["id"], "newer");
        assert_eq!(value["reports"][0]["summary"], "Service warning");
        assert!(!value.to_string().contains("private"));
        assert!(!value.to_string().contains("secret-source"));
        assert_eq!(
            store
                .native_overview("issuer#admin", "Admin", 200000)
                .unwrap()["stale"],
            true
        );
    }
}
