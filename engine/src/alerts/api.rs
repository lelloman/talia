//! Same authenticated API for MCP agents and trusted platform hosts.
use super::{delivery::*, policy::*, *};
use crate::authority::{Action, Session};
fn parsed<T: DeserializeOwned>(v: &Value) -> Result<T> {
    serde_json::from_value(v.clone()).map_err(|_| "invalid input".into())
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or_else(|| format!("{k} required"))
}
fn integer(v: &Value, k: &str) -> Result<u64> {
    v[k].as_u64()
        .filter(|n| *n < 9_007_199_254_740_000)
        .ok_or_else(|| format!("{k} required"))
}
fn fields(a: &Value, allowed: &[&str]) -> Result<()> {
    if a.as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        Err("invalid input fields".into())
    } else {
        Ok(())
    }
}
fn permission(op: &str) -> Result<Action> {
    Ok(match op {
        "snapshot" | "history" => Action::Read,
        "audit" => Action::Audit,
        "config" | "policy_save" | "binding_save" | "destination_save" | "device_groups" => {
            Action::Configure
        }
        "acknowledge" => Action::Acknowledge,
        "silence_save" => Action::Silence,
        "observe" => Action::Observe,
        "device_register" | "device_status" => Action::Register,
        _ => return Err("unknown alert operation".into()),
    })
}
impl Store {
    pub fn alert_api(
        &mut self,
        session: &Session,
        op: &str,
        args: Value,
        now: i64,
    ) -> Result<Value> {
        let permission = permission(op)?;
        self.alert_require(session, permission)
            .map_err(|e| format!("{e:?}").to_lowercase())?;
        let actor = session.principal();
        if ["snapshot", "history", "audit", "config", "device_status"].contains(&op) {
            return self.alert_api_inner(op, &args, actor, now);
        }
        let request = text(&args, "requestId")?;
        key(request)?;
        let id = format!("{}:{request}", actor);
        let signature: String = ring::digest::digest(
            &ring::digest::SHA256,
            json!([op, args]).to_string().as_bytes(),
        )
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
        self.alert_atomic(|s| {
            let previous: Option<(String, String)> = s
                .conn
                .query_row(
                    "SELECT signature,result FROM alert_requests WHERE id=?",
                    [&id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(err)?;
            if let Some((old, result)) = previous {
                if old != signature {
                    return Err("request conflict".into());
                }
                return serde_json::from_str(&result).map_err(err);
            }
            let n: u64 = s
                .conn
                .query_row("SELECT count(*) FROM alert_requests", [], |r| r.get(0))
                .map_err(err)?;
            if n >= 10000 {
                return Err("request capacity".into());
            }
            let result = s.alert_api_inner(op, &args, actor, now)?;
            s.conn
                .execute(
                    "INSERT INTO alert_requests VALUES(?,?,?)",
                    params![id, signature, result.to_string()],
                )
                .map_err(err)?;
            Ok(result)
        })
    }
    fn alert_api_inner(&mut self, op: &str, a: &Value, actor: &str, now: i64) -> Result<Value> {
        match op {
            "snapshot" => {
                fields(a, &[])?;
                let evaluations:Vec<Value>=self.alert_evaluations()?.into_iter().map(|e|json!({"id":e["id"],"error":e["evaluation"]["error"],"last_at":e["evaluation"]["last_at"]})).collect();
                Ok(
                    json!({"alerts":self.alerts()?,"deliveries":self.alert_deliveries()?,"silences":self.alert_silences()?,"evaluations":evaluations,"now":now}),
                )
            }
            "history" => {
                fields(a, &["key"])?;
                Ok(json!({"occurrences":self.alert_history(text(a,"key")?)?}))
            }
            "audit" => {
                fields(a, &["key"])?;
                Ok(json!({"events":self.alert_audit_history(a["key"].as_str())?}))
            }
            "config" => {
                fields(a, &[])?;
                Ok(
                    json!({"policies":self.alert_policies()?,"bindings":self.alert_bindings()?,"destinations":self.alert_destinations()?,"devices":self.alert_devices_public()?}),
                )
            }
            "acknowledge" => {
                fields(a, &["requestId", "key", "occurrence", "expected"])?;
                Ok(
                    json!({"alert":self.alert_acknowledge(text(a,"key")?,integer(a,"occurrence")?,integer(a,"expected")?,actor,now)?}),
                )
            }
            "observe" => {
                fields(a, &["requestId", "observation", "expected"])?;
                let o: Observation = parsed(&a["observation"])?;
                Ok(json!({"alert":self.alert_observe(&o,integer(a,"expected")?,actor,now)?}))
            }
            "policy_save" => {
                fields(a, &["requestId", "policy", "expected", "migrations"])?;
                let p: Policy = parsed(&a["policy"])?;
                let migrations = if a.get("migrations").is_some() {
                    parsed(&a["migrations"])?
                } else {
                    BTreeMap::new()
                };
                self.alert_policy_save(&p, integer(a, "expected")?, &migrations, actor, now)?;
                Ok(json!({"version":p.version}))
            }
            "binding_save" => {
                fields(a, &["requestId", "binding", "expected"])?;
                let b: Binding = parsed(&a["binding"])?;
                self.alert_binding_save(&b, integer(a, "expected")?, actor, now)?;
                Ok(json!({"version":b.version}))
            }
            "destination_save" => {
                fields(a, &["requestId", "destination", "expected"])?;
                let d: Destination = parsed(&a["destination"])?;
                self.alert_destination_save(&d, integer(a, "expected")?, actor, now)?;
                Ok(json!({"version":d.version}))
            }
            "silence_save" => {
                fields(a, &["requestId", "silence", "expected"])?;
                let silence: Silence = parsed(&a["silence"])?;
                self.alert_silence_save(&silence, integer(a, "expected")?, actor, now)?;
                Ok(json!({"version":silence.version}))
            }
            "device_status" => {
                fields(a, &["id"])?;
                let device = self.alert_get::<Device>("device", text(a, "id")?)?;
                if device.as_ref().is_some_and(|d| d.owner != actor) {
                    return Err("forbidden".into());
                }
                Ok(
                    json!({"device":device.map(|d|json!({"id":d.id,"version":d.version,"enabled":d.enabled}))}),
                )
            }
            "device_register" => {
                fields(a, &["requestId", "device", "expected"])?;
                let d: Device = parsed(&a["device"])?;
                self.alert_device_register(&d, integer(a, "expected")?, actor, now)?;
                Ok(json!({"id":d.id,"version":d.version}))
            }
            "device_groups" => {
                fields(a, &["requestId", "id", "expected", "groups"])?;
                let expected = integer(a, "expected")?;
                self.alert_device_groups(
                    text(a, "id")?,
                    expected,
                    parsed(&a["groups"])?,
                    actor,
                    now,
                )?;
                Ok(json!({"version":expected+1}))
            }
            _ => Err("unknown alert operation".into()),
        }
    }
}
pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let number = json!({"type":"integer","minimum":0});
    let mut tools = vec![];
    for(op,description,mut properties,required,read)in [
 ("snapshot","Read current alerts, acknowledgement, silences and sanitized delivery/evaluation status.",json!({}),vec![],true),
 ("history","Read occurrence history for one stable alert key.",json!({"key":string}),vec!["key"],true),
 ("audit","Read alert audit events; requires audit permission.",json!({"key":string}),vec![],true),
 ("config","Read policies, bindings, destinations and token-free device metadata; requires configure permission.",json!({}),vec![],true),
 ("acknowledge","Globally acknowledge an exact occurrence/revision. Does not resolve it. Reuse requestId only for identical retries.",json!({"key":string,"occurrence":number,"expected":number}),vec!["key","occurrence","expected"],false),
 ("observe","Raise/update/resolve an alert using a stable key and expected revision. Prefer automatic policy bindings for monitored inputs.",json!({"observation":{"type":"object","description":"{key,active,stage,severity,message,labels?:object,reset_ack?:boolean}"},"expected":number}),vec!["observation","expected"],false),
 ("policy_save","Save a shared staged policy. Source is a JS object expression with async evaluate(ctx); ctx.read(alias), ctx.state, ctx.params, ctx.alert and ctx.now(). Return {active,stage,severity,message,actions?}. Removing an occupied stage requires migrations.",json!({"policy":{"type":"object","description":"{id,version,source,stages:{name:{reset_ack?,actions:[{id,destinations,delay_ms?,repeat_ms?,until_ack?,max_attempts?,retry_ms?,expiry_ms?}]}},recovery?:[action]}"},"expected":number,"migrations":{"type":"object","additionalProperties":{"type":"string"}}}),vec!["policy","expected"],false),
 ("binding_save","Reference a policy with independent parameters, named engine inputs and stable alert key. Identity/policy reference are immutable; updates use expected version.",json!({"binding":{"type":"object","description":"{id,version,policy,key,params?,inputs?:{alias:variableId},labels?,every_ms?,enabled?}"},"expected":number}),vec!["binding","expected"],false),
 ("destination_save","Save a named email/telegram/push destination referencing an operator-owned provider. Push targets device:ID, user:OWNER or group:NAME.",json!({"destination":{"type":"object","description":"{id,version,channel,provider,target,enabled?}"},"expected":number}),vec!["destination","expected"],false),
 ("silence_save","Create/update an expiring key/label silence. Set until to current server time to end it. Actor comes from authentication.",json!({"silence":{"type":"object","description":"{id,version,key?,labels?,until,reason}"},"expected":number}),vec!["silence","expected"],false),
 ("device_status","Read this principal's installation revision without exposing its token; requires register permission.",json!({"id":string}),vec!["id"],true),
 ("device_register","Register/rotate this principal's installation push address. Owner comes from authentication; groups cannot be self-assigned.",json!({"device":{"type":"object","description":"{id,version,owner,token,enabled?}"},"expected":number}),vec!["device","expected"],false),
 ("device_groups","Set device groups with configure permission and expected device revision.",json!({"id":string,"groups":{"type":"array","items":{"type":"string"},"maxItems":32},"expected":number}),vec!["id","groups","expected"],false)
 ] {let mut required=required;if !read {properties["requestId"]=string.clone();required.push("requestId");}tools.push(json!({"name":format!("alerts_{op}"),"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":true,"openWorldHint":!read}}));}
    tools
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{Family, Grant, Scope};
    #[test]
    fn authenticated_operations_replay_and_redaction() {
        let mut s = Store::open(":memory:").unwrap();
        s.agent_policy_set(
            "operator",
            0,
            true,
            &[Grant {
                family: Family::Alerts,
                actions: [Action::Read, Action::Observe, Action::Acknowledge]
                    .into_iter()
                    .collect(),
                scope: Scope::All,
            }],
        )
        .unwrap();
        let t = s.agent_credential_issue("operator").unwrap();
        let session = s.agent_authenticate(&t).unwrap();
        let observation = json!({"requestId":"open","expected":0,"observation":{"key":"disk","active":true,"stage":"warning","severity":"warning","message":"low"}});
        let result = s
            .alert_api(&session, "observe", observation.clone(), 0)
            .unwrap();
        assert_eq!(
            s.alert_api(&session, "observe", observation, 1).unwrap(),
            result
        );
        let ack = json!({"requestId":"ack","key":"disk","occurrence":1,"expected":1});
        let result = s
            .alert_api(&session, "acknowledge", ack.clone(), 2)
            .unwrap();
        assert_eq!(
            s.alert_api(&session, "acknowledge", ack, 3).unwrap(),
            result
        );
        assert!(s.alert_api(&session, "config", json!({}), 4).is_err());
        assert!(s
            .alert_api(&session, "snapshot", json!({"unexpected":true}), 4)
            .is_err());
        assert!(!s
            .alert_api(&session, "snapshot", json!({}), 4)
            .unwrap()
            .to_string()
            .contains(&t));
        s.agent_credential_revoke(&t).unwrap();
        assert!(s.alert_api(&session, "snapshot", json!({}), 4).is_err());
    }
}
