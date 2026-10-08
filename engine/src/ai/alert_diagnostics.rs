//! Read-only, bounded alert evidence for investigations. Never return destinations,
//! credentials, policy source, binding parameters or arbitrary evaluation state.
use super::*;
use crate::alerts::{
    delivery::Delivery,
    policy::{Binding, Evaluation, Policy},
};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Section {
    Overview,
    History,
    Deliveries,
    Audit,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    key: String,
    section: Section,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    20
}
fn short(text: &str) -> String {
    text.chars().take(1000).collect()
}
fn alert(a: crate::alerts::Alert) -> Value {
    json!({"key":a.key,"occurrence":a.occurrence,"revision":a.revision,"active":a.active,
        "stage":a.stage,"severity":a.severity,"message":short(&a.message),
        "message_truncated":a.message.chars().count()>1000,"opened_at":a.opened_at,
        "stage_at":a.stage_at,"updated_at":a.updated_at,"resolved_at":a.resolved_at,
        "acknowledgement":a.acknowledgement})
}
fn actions(items: &[crate::alerts::policy::ResponseAction]) -> Vec<Value> {
    items
        .iter()
        .map(|a| {
            json!({"id":a.id,"delay_ms":a.delay_ms,"repeat_ms":a.repeat_ms,
        "until_ack":a.until_ack,"retry_ms":a.retry_ms,"max_attempts":a.max_attempts,
        "expiry_ms":a.expiry_ms})
        })
        .collect()
}
pub(super) fn inspect(engine: &Engine, args: Value) -> Result<Value> {
    let r: Request =
        serde_json::from_value(args).map_err(|_| "invalid alert inspection arguments")?;
    crate::alerts::key(&r.key)?;
    if !(1..=50).contains(&r.limit) || r.offset > 10000 {
        return Err("limit must be 1–50 and offset 0–10000".into());
    }
    let s = engine.store.borrow();
    let current = s.alert(&r.key)?;
    let (sql, kind) = match r.section {
        Section::Overview => ("SELECT body FROM alert_entities WHERE kind=? AND json_extract(body,'$.key')=? ORDER BY id LIMIT ? OFFSET ?", "binding"),
        Section::History => ("SELECT body FROM alert_entities WHERE kind=? AND json_extract(body,'$.key')=? ORDER BY json_extract(body,'$.occurrence') DESC,id DESC LIMIT ? OFFSET ?", "occurrence"),
        Section::Deliveries => ("SELECT body FROM alert_entities WHERE kind=? AND json_extract(body,'$.key')=? ORDER BY json_extract(body,'$.updated_at') DESC,id DESC LIMIT ? OFFSET ?", "delivery"),
        Section::Audit => ("SELECT json_object('id',seq,'at',at,'actor',actor,'operation',operation,'detail',json(body)) FROM alert_audit WHERE ?='audit' AND entity=? ORDER BY seq DESC LIMIT ? OFFSET ?", "audit"),
    };
    let mut query = s.conn.prepare(sql).map_err(err)?;
    let rows = query
        .query_map(params![kind, r.key, r.limit + 1, r.offset], |row| {
            row.get::<_, String>(0)
        })
        .map_err(err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut more = rows.len() > r.limit;
    let mut items = Vec::new();
    for row in rows.iter().take(r.limit) {
        let item = match r.section {
            Section::Overview => {
                let b: Binding = serde_json::from_str(row).map_err(err)?;
                let p = s.alert_get::<Policy>("policy", &b.policy)?;
                let e = s.alert_get::<Evaluation>("evaluation", &b.id)?;
                let stage = p.as_ref().and_then(|p| p.stages.get(&current.stage));
                let selected = if current.active {
                    stage.map(|v| v.actions.as_slice()).unwrap_or_default()
                } else {
                    p.as_ref()
                        .map(|p| p.recovery.as_slice())
                        .unwrap_or_default()
                };
                json!({"binding":b.id,"enabled":b.enabled,"binding_version":b.version,
                    "policy":b.policy,"policy_version":p.as_ref().map(|p|p.version),
                    "inputs":b.inputs,"every_ms":b.every_ms,"reset_ack_on_stage":stage.map(|s|s.reset_ack),
                    "actions":actions(selected),"evaluation":e.map(|e|json!({"last_at":e.last_at,
                        "next_at":e.next_at,"error":e.error.map(|v|short(&v)),
                        "binding_version":e.binding_version,"policy_version":e.policy_version}))})
            }
            Section::History => alert(serde_json::from_str(row).map_err(err)?),
            Section::Deliveries => {
                let d: Delivery = serde_json::from_str(row).map_err(err)?;
                json!({"id":d.id,"binding":d.binding,"occurrence":d.occurrence,"stage":d.stage,
                    "stage_at":d.stage_at,"active":d.active,"status":d.status,"attempts":d.attempts,
                    "due":d.due,"expires":d.expires,"updated_at":d.updated_at,"error":d.error.map(|e|short(&e)),
                    "binding_version":d.binding_version,"policy_version":d.policy_version,
                    "action":actions(&[d.action]).remove(0)})
            }
            Section::Audit => {
                let v: Value = serde_json::from_str(row).map_err(err)?;
                let mut detail = serde_json::Map::new();
                for k in [
                    "occurrence",
                    "revision",
                    "delivery",
                    "attempts",
                    "version",
                    "until",
                ] {
                    if let Some(value) = v["detail"].get(k) {
                        detail.insert(k.into(), value.clone());
                    }
                }
                json!({"id":v["id"],"at":v["at"],"actor":v["actor"],"operation":v["operation"],"detail":detail})
            }
        };
        items.push(item);
        // A single overview can contain 32 actions and inputs; bound the full reply.
        if serde_json::to_vec(&items).map_err(err)?.len() > 24000 {
            items.pop();
            more = true;
            if items.is_empty() {
                return Err("alert inspection record exceeds response limit".into());
            }
            break;
        }
    }
    Ok(
        json!({"key":r.key,"now":engine.now(),"timestamps":"Unix milliseconds",
        "current":alert(current.clone()),"silenced_now":s.alert_silenced(&current,engine.now())?,
        "items":items,"next_offset":if more {Some(r.offset+items.len())} else {None},
        "note":"Retained evidence only. Delivery updated_at is the latest recorded status time, not a recipient read receipt. Current policy can differ from historical delivery action settings. Pagination is a live view."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::{delivery::Outcome, Observation};
    use std::rc::Rc;
    #[test]
    fn evidence_tracks_real_deliveries_and_ack_without_exposing_destinations() {
        let mut s = crate::alerts::delivery::tests::fixture();
        s.alert_schedule(0).unwrap();
        let first = s.alert_deliveries().unwrap().remove(0);
        s.alert_claim_delivery(&first.id, 0).unwrap().unwrap();
        s.alert_finish_delivery(&first.id, Outcome::Sent, 20)
            .unwrap();
        let a = s.alert("disk").unwrap();
        s.alert_acknowledge("disk", a.occurrence, a.revision, "reviewer", 50)
            .unwrap();
        let e = Engine::with_clock(s, Rc::new(|| 100));
        let overview = inspect(&e, json!({"key":"disk","section":"overview"})).unwrap();
        assert_eq!(overview["current"]["acknowledgement"]["actor"], "reviewer");
        assert_eq!(overview["items"][0]["actions"][0]["repeat_ms"], 1000);
        let deliveries =
            inspect(&e, json!({"key":"disk","section":"deliveries","limit":1})).unwrap();
        assert_eq!(deliveries["items"][0]["status"], "sent");
        assert_eq!(deliveries["items"][0]["updated_at"], 20);
        assert_eq!(deliveries["next_offset"], 1);
        let next = inspect(
            &e,
            json!({"key":"disk","section":"deliveries","offset":1,"limit":1}),
        )
        .unwrap();
        assert!(next["next_offset"].is_null());
        assert_ne!(next["items"][0]["id"], deliveries["items"][0]["id"]);
        for data in [&overview, &deliveries, &next] {
            let text = data.to_string();
            for secret in [
                "@example.test",
                "smtp",
                "\"destinations\"",
                "\"source\"",
                "\"params\"",
                "\"device\"",
            ] {
                assert!(!text.contains(secret), "unexpected field: {secret}");
            }
        }
        let audit = inspect(&e, json!({"key":"disk","section":"audit"})).unwrap();
        assert_eq!(audit["items"][0]["operation"], "acknowledged");
        assert_eq!(audit["items"][0]["actor"], "reviewer");
    }
    #[test]
    fn history_orders_occurrences_and_reports_truncation_and_invalid_inputs() {
        let mut s = Store::open(":memory:").unwrap();
        for i in 0..12 {
            for active in [true, false] {
                let expected = s.alert("a").map_or(0, |a| a.revision);
                s.alert_observe(
                    &Observation {
                        key: "a".into(),
                        active,
                        stage: "firing".into(),
                        severity: "warning".into(),
                        message: "x".repeat(2000),
                        labels: Default::default(),
                        reset_ack: false,
                    },
                    expected,
                    "monitor",
                    i,
                )
                .unwrap();
            }
        }
        let e = Engine::with_clock(s, Rc::new(|| 100));
        let v = inspect(&e, json!({"key":"a","section":"history","limit":2})).unwrap();
        assert_eq!(v["items"][0]["occurrence"], 12);
        assert_eq!(v["items"][1]["occurrence"], 11);
        assert_eq!(v["items"][0]["message_truncated"], true);
        assert_eq!(v["next_offset"], 2);
        assert!(v.to_string().len() < 32768);
        for bad in [
            json!({"key":"a","section":"audit","limit":0}),
            json!({"key":"a","section":"audit","limit":51}),
            json!({"key":"a","section":"config"}),
            json!({"key":"missing","section":"history"}),
            json!({"key":"a","section":"audit","sql":"SELECT 1"}),
        ] {
            assert!(inspect(&e, bad).is_err());
        }
    }
}
