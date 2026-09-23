use super::*;
use crate::authority::Session;
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str().ok_or_else(|| format!("{k} required"))
}
fn number(v: &Value, k: &str) -> Result<u64> {
    v[k].as_u64()
        .filter(|n| *n < 9_007_199_254_740_000)
        .ok_or_else(|| format!("{k} required"))
}
impl Store {
    pub fn report_api(
        &mut self,
        session: &Session,
        op: &str,
        args: Value,
        now: i64,
    ) -> Result<Value> {
        self.agent_require_dashboard_admin(session)
            .map_err(|_| "forbidden")?;
        let fields: &[&str] = match op {
            "list" => &[],
            "get" => &["id"],
            "runs" => &["report", "before", "limit"],
            "run_get" => &["id"],
            "save" => &["definition", "expected", "requestId"],
            "run" => &["id", "send", "requestId"],
            "deliver" => &["id", "requestId"],
            "prune" => &["before", "requestId"],
            _ => return Err("unknown report operation".into()),
        };
        if args
            .as_object()
            .is_none_or(|a| a.keys().any(|k| !fields.contains(&k.as_str())))
        {
            return Err("unknown report fields".into());
        }
        if ["list", "get", "runs", "run_get"].contains(&op) {
            return self.report_inner(op, &args, session.principal(), now);
        }
        let request = text(&args, "requestId")?;
        id(request)?;
        let key = format!("{}:{request}", session.principal());
        let signature = ring::digest::digest(
            &ring::digest::SHA256,
            json!([op, args]).to_string().as_bytes(),
        )
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
        self.alert_atomic(|s| {
            let old: Option<(String, String)> = s
                .conn
                .query_row(
                    "SELECT signature,result FROM report_requests WHERE id=?",
                    [&key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(err)?;
            if let Some((sig, result)) = old {
                if sig != signature {
                    return Err("request conflict".into());
                }
                return serde_json::from_str(&result).map_err(err);
            }
            let result = s.report_inner(op, &args, session.principal(), now)?;
            s.conn
                .execute(
                    "INSERT INTO report_requests VALUES(?,?,?)",
                    params![key, signature, result.to_string()],
                )
                .map_err(err)?;
            Ok(result)
        })
    }
    fn report_inner(&mut self, op: &str, args: &Value, actor: &str, now: i64) -> Result<Value> {
        match op {
            "list" => Ok(json!({"definitions":self.report_definitions()?})),
            "get" => Ok(json!({"definition":self.report_definition(text(args,"id")?)?})),
            "save" => {
                let d: Definition = serde_json::from_value(args["definition"].clone())
                    .map_err(|_| "invalid report definition")?;
                self.report_save(&d, number(args, "expected")?, now)?;
                Ok(json!({"id":d.id,"version":d.version}))
            }
            "run" => {
                let send = args["send"]
                    .as_bool()
                    .ok_or("send boolean required; false previews without delivery")?;
                let r = self.report_start(text(args, "id")?, actor, send, now)?;
                Ok(json!({"run_id":r.id,"status":r.status}))
            }
            "run_get" => Ok(json!({"run":self.report_run(text(args,"id")?)?})),
            "runs" => {
                let report = text(args, "report")?;
                let limit = args["limit"].as_u64().unwrap_or(20);
                if limit == 0 || limit > 100 {
                    return Err("limit must be 1..100".into());
                }
                let before = args["before"].as_str().unwrap_or("~");
                let mut q=self.conn.prepare("SELECT body FROM report_runs WHERE report=? AND id<? ORDER BY id DESC LIMIT ?").map_err(err)?;
                let rows = q
                    .query_map(params![report, before, limit], |r| r.get::<_, String>(0))
                    .map_err(err)?;
                let runs=rows.map(|r|{let r:Run=serde_json::from_str(&r.map_err(err)?).map_err(err)?;Ok(json!({"id":r.id,"created":r.created,"status":r.status,"version":r.definition.version,"send":r.send,"error":r.error,"deliveries":r.deliveries}))}).collect::<Result<Vec<_>>>()?;
                Ok(
                    json!({"runs":runs,"next_before":if runs.len()==limit as usize{runs.last().map(|r|r["id"].clone())}else{None}}),
                )
            }
            "deliver" => {
                let mut r = self.report_run(text(args, "id")?)?;
                if r.definition.destinations.is_empty() {
                    return Err("email or Telegram destination required for sending".into());
                }
                if r.send
                    || r.content.is_none()
                    || !["complete", "partial"].contains(&r.status.as_str())
                {
                    return Err("only an unsent completed preview can be delivered".into());
                }
                let destinations = self.alert_destinations()?;
                for name in &r.definition.destinations {
                    let d = destinations
                        .iter()
                        .find(|d| {
                            &d.id == name
                                && d.enabled
                                && ["email", "telegram"].contains(&d.channel.as_str())
                        })
                        .ok_or("email or Telegram destination unavailable")?;
                    r.deliveries.push(Delivery {
                        destination: name.clone(),
                        version: d.version,
                        status: "pending".into(),
                        attempts: 0,
                        next_at: 0,
                        error: None,
                    });
                }
                r.send = true;
                r.status = "delivering".into();
                r.deadline = now.checked_add(300000).ok_or("deadline overflow")?;
                self.report_put(&r)?;
                Ok(json!({"run_id":r.id,"status":r.status}))
            }
            "prune" => {
                let before = number(args, "before")? as i64;
                if before > now {
                    return Err("prune cutoff is in the future".into());
                }
                let n=self.conn.execute("DELETE FROM report_runs WHERE created<? AND status IN ('complete','partial','failed')",[before]).map_err(err)?;
                Ok(json!({"removed":n,"request_tombstones_retained":true}))
            }
            _ => Err("unknown report operation".into()),
        }
    }
}
pub fn tools() -> Vec<Value> {
    let mut tools = vec![];
    for (op,description,properties,required) in [
  ("list","List server-side reporting workflow definitions. Requires global authoring/admin access.",json!({}),vec![]),
  ("get","Read one versioned reporting definition.",json!({"id":{"type":"string"}}),vec!["id"]),
  ("save","Create/update a reporting workflow without restart. Ordered steps support read, source, script. JavaScript is a synchronous function(ctx), with ctx.steps, ctx.period, ctx.now and ctx.decode(wire). compose returns {subject,summary,sections:[{title,text}]}; HTML is escaped.",json!({"definition":{"type":"object","description":"{id,version,enabled,schedule:null|{kind:daily,time:HH:MM,zone:IANA,weekdays?:[1..7]}|{kind:interval,every_ms},period_ms?,timeout_ms?,steps:[{id,optional?,kind:read,variable}|{id,optional?,kind:source,source,request:JS}|{id,optional?,kind:script,source:JS}],compose:JS,destinations:[emailOrTelegramDestinationId]}"},"expected":{"type":"integer","minimum":0}}),vec!["definition","expected"]),
  ("run","Run a report now. send:false executes all steps and saves a preview without delivery; send:true also delivers. One execution per report at a time. Reuse requestId only for an identical retry.",json!({"id":{"type":"string"},"send":{"type":"boolean"}}),vec!["id","send"]),
  ("run_get","Read frozen definition, step results, tracked agent session, report HTML/text and per-destination delivery outcomes for one run.",json!({"id":{"type":"string"}}),vec!["id"]),
  ("runs","List bounded run summaries for a report, paginated by exclusive run-ID cursor.",json!({"report":{"type":"string"},"before":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100}}),vec!["report"]),
  ("deliver","Deliver an already-composed unsent preview without rerunning checks. An already requested delivery cannot be replayed with a different key.",json!({"id":{"type":"string"}}),vec!["id"]),
  ("prune","Delete terminal run content before UTC millisecond cutoff. Request tombstones remain to prevent replay starting new work.",json!({"before":{"type":"integer","minimum":0}}),vec!["before"]),
 ] {let read=["list","get","runs","run_get"].contains(&op);let mut props=properties;let mut req=required;if !read{props["requestId"]=json!({"type":"string","minLength":1,"maxLength":64});req.push("requestId");}tools.push(json!({"name":format!("reports_{op}"),"description":description,"inputSchema":{"type":"object","properties":props,"required":req,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":op=="prune","idempotentHint":true,"openWorldHint":!read}}));}
    tools
}
