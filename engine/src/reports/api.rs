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
        self.report_authorized(op, args, session.principal(), now)
    }
    fn report_authorized(&mut self, op: &str, args: Value, actor: &str, now: i64) -> Result<Value> {
        let fields: &[&str] = match op {
            "list" => &[],
            "get" | "schedule_get" => &["id"],
            "schedule_save" => &["id", "enabled", "schedule", "expected", "requestId"],
            "runs" => &["report", "before", "limit", "sort", "status"],
            "run_get" | "analysis_get" => &["id"],
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
        if ["list", "get", "runs", "run_get", "analysis_get", "schedule_get"].contains(&op) {
            return self.report_inner(op, &args, actor, now);
        }
        let request = text(&args, "requestId")?;
        id(request)?;
        let key = format!("{}:{request}", actor);
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
            let result = s.report_inner(op, &args, actor, now)?;
            s.conn
                .execute(
                    "INSERT INTO report_requests VALUES(?,?,?)",
                    params![key, signature, result.to_string()],
                )
                .map_err(err)?;
            Ok(result)
        })
    }
    /// Trusted native HTTP host supplies the authenticated subject, never client arguments.
    pub fn native_reports(&mut self, subject: &str, op: &str, args: Value, now: i64) -> Result<Value> {
        if !self.user_admin(subject).map_err(|_| "forbidden")? {
            return Err("forbidden".into());
        }
        if !["list", "runs", "run_get", "run", "schedule_get", "schedule_save"].contains(&op) {
            return Err("unknown report operation".into());
        }
        // Native runs are retained previews; delivery is a separate operator action.
        if op == "run" && args["send"] != false {
            return Err("native runs require send:false".into());
        }
        let value = self.report_authorized(op, args, &format!("native:{subject}"), now)?;
        if op == "list" {
            let mut definitions = vec![];
            for d in self.report_definitions()? {
                let latest = self.report_inner("runs", &json!({"report":d.id,"limit":1}), subject, now)?;
                definitions.push(json!({"id":d.id,"enabled":d.enabled,"scheduled":d.schedule.is_some(),
                    "steps":d.steps.len(),"period_ms":d.period_ms,
                    "version":d.version,"schedule":d.schedule,"next_due":self.report_schedule_view(&d)?["next_due"],
                    "available":!d.steps.iter().any(|s| matches!(s.action, Action::Unavailable {..})),
                    "latest":latest["runs"][0]}));
            }
            definitions.sort_by(|a, b| b["latest"]["created"].as_i64().cmp(&a["latest"]["created"].as_i64())
                .then_with(|| a["id"].as_str().cmp(&b["id"].as_str())));
            return Ok(json!({"definitions":definitions}));
        }
        if op == "run_get" {
            let r = &value["run"];
            let steps: Vec<Value> = r["definition"]["steps"].as_array().into_iter().flatten().enumerate().map(|(index, step)| {
                let output = &r["outputs"][step["id"].as_str().unwrap_or("")];
                let fallback = if r["status"] == "running" && r["index"].as_u64() == Some(index as u64) { "running" }
                    else if r["status"] == "failed" { "not_run" } else { "pending" };
                json!({"id":step["id"],"status":output["status"].as_str().unwrap_or(fallback),"error":output["error"]})
            }).collect();
            return Ok(json!({"run":{"id":r["id"],"report":r["definition"]["id"],"created":r["created"],
                "period_start":r["period_start"],"status":r["status"],"send":r["send"],
                "steps":steps,"content":r["content"],"error":r["error"],"deliveries":r["deliveries"]}}));
        }
        Ok(value)
    }
    fn report_schedule_view(&self, d: &Definition) -> Result<Value> {
        let due: Option<i64> = self.conn.query_row("SELECT next_due FROM report_definitions WHERE id=?", [&d.id], |r| r.get(0)).map_err(err)?;
        Ok(json!({"id":d.id,"version":d.version,"enabled":d.enabled,"schedule":d.schedule,
            "next_due":due,"destinations":d.destinations}))
    }
    fn report_inner(&mut self, op: &str, args: &Value, actor: &str, now: i64) -> Result<Value> {
        match op {
            "list" => Ok(json!({"definitions":self.report_definitions()?})),
            "get" => Ok(json!({"definition":self.report_definition(text(args,"id")?)?})),
            "schedule_get" => self.report_schedule_view(&self.report_definition(text(args, "id")?)?),
            "schedule_save" => {
                let mut d = self.report_definition(text(args, "id")?)?;
                let expected = number(args, "expected")?;
                if d.version != expected { return Err("report version conflict; reload the schedule".into()); }
                d.enabled = args["enabled"].as_bool().ok_or("enabled boolean required")?;
                if !args.as_object().unwrap().contains_key("schedule") { return Err("schedule required".into()); }
                d.schedule = serde_json::from_value(args["schedule"].clone()).map_err(|_| "invalid schedule")?;
                if d.enabled && d.schedule.is_none() { return Err("choose a schedule before enabling".into()); }
                d.version = expected.checked_add(1).ok_or("version overflow")?;
                self.report_save(&d, expected, now)?;
                self.report_schedule_view(&d)
            }
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
            "analysis_get" => {
                Ok(json!({"run":self.ai_run(text(args,"id")?)?.ok_or("AI run not found")?}))
            }
            "run_get" => Ok(json!({"run":self.report_run(text(args,"id")?)?})),
            "runs" => {
                let report = text(args, "report")?;
                let limit = args["limit"].as_u64().unwrap_or(20);
                if limit == 0 || limit > 100 {
                    return Err("limit must be 1..100".into());
                }
                let sort = args["sort"].as_str().unwrap_or("newest");
                if !["newest", "oldest"].contains(&sort) { return Err("invalid history sort".into()); }
                let status = args["status"].as_str().unwrap_or("all");
                if !["all", "active", "complete", "issues", "failed"].contains(&status) { return Err("invalid history status".into()); }
                let (before_created, before_id) = if let Some(before) = args["before"].as_str() {
                    let created: i64 = self.conn.query_row(
                        "SELECT created FROM report_runs WHERE report=? AND id=?", params![report, before], |r| r.get(0)
                    ).optional().map_err(err)?.ok_or("history cursor expired; refresh history")?;
                    (created, before)
                } else if sort == "oldest" { (i64::MIN, "") } else { (i64::MAX, "~") };
                let (comparison, direction) = if sort == "oldest" { (">", "ASC") } else { ("<", "DESC") };
                // Only validated, fixed SQL fragments are interpolated. User values stay bound.
                let sql = format!("SELECT body FROM report_runs WHERE report=? AND (created,id){comparison}(?,?)
                    AND (?='all' OR (?='active' AND status IN ('queued','running','delivering'))
                    OR (?='issues' AND status IN ('partial','failed')) OR status=?)
                    ORDER BY created {direction},id {direction} LIMIT ?");
                let mut q = self.conn.prepare(&sql).map_err(err)?;
                let rows = q.query_map(params![report, before_created, before_id, status, status, status, status, limit], |r| r.get::<_, String>(0)).map_err(err)?;
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
                let ai=self.conn.execute("DELETE FROM ai_runs WHERE created<? AND status IN ('complete','failed') AND NOT EXISTS(SELECT 1 FROM report_runs r WHERE r.id=json_extract(ai_runs.body,'$.scope.run') AND r.status IN ('queued','running','delivering')) AND NOT EXISTS(SELECT 1 FROM telegram_jobs j WHERE j.id=json_extract(ai_runs.body,'$.scope.job') AND j.status IN ('queued','running'))",[before]).map_err(err)?;
                Ok(json!({"removed":n,"ai_removed":ai,"request_tombstones_retained":true}))
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
  ("schedule_get","Read report schedule, version, configured destination IDs and stored next due time.",json!({"id":{"type":"string"}}),vec!["id"]),
  ("schedule_save","Edit only report timing and enabled state; preserve steps and destinations. Requires the version read by schedule_get. Pausing leaves admitted runs running. Reuse requestId for identical retries.",json!({"id":{"type":"string"},"expected":{"type":"integer","minimum":1},"enabled":{"type":"boolean"},"schedule":{"description":"null or {kind:daily,time:HH:MM,zone:IANA,weekdays?:[1..7]} or {kind:interval,every_ms} (minimum 60000 ms)","type":["object","null"]}}),vec!["id","expected","enabled","schedule"]),
  ("save","Create/update a reporting workflow without restart. Ordered steps support read, source, script, analysis. Analysis uses simple-ai with explicit previous-step inputs and no tools. JavaScript is a synchronous function(ctx), with ctx.steps, ctx.period, ctx.now and ctx.decode(wire). compose returns {subject,summary,sections:[{title,text}]}; HTML is escaped.",json!({"definition":{"type":"object","description":"{id,version,enabled,schedule:null|{kind:daily,time:HH:MM,zone:IANA,weekdays?:[1..7]}|{kind:interval,every_ms},period_ms?,timeout_ms?,steps:[{id,optional?,kind:read,variable}|{id,optional?,kind:source,source,request:JS}|{id,optional?,kind:script,source:JS}|{id,optional?,kind:analysis,instructions,inputs:[stepId]}],compose:JS,destinations:[emailOrTelegramDestinationId]}"},"expected":{"type":"integer","minimum":0}}),vec!["definition","expected"]),
  ("run","Run a report now. send:false executes all steps and saves a preview without delivery; send:true also delivers. One execution per report at a time. Reuse requestId only for an identical retry.",json!({"id":{"type":"string"},"send":{"type":"boolean"}}),vec!["id","send"]),
  ("analysis_get","Inspect a retained Talìa AI run, including bounded messages, tool results, model, usage and errors. Administrator only. Includes report and Telegram runs.",json!({"id":{"type":"string"}}),vec!["id"]),
  ("run_get","Read frozen definition, step results, report HTML/text and per-destination delivery outcomes for one run.",json!({"id":{"type":"string"}}),vec!["id"]),
  ("runs","List bounded run summaries for a report, paginated by exclusive run-ID cursor.",json!({"report":{"type":"string"},"before":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100},"sort":{"enum":["newest","oldest"]},"status":{"enum":["all","active","complete","issues","failed"]}}),vec!["report"]),
  ("deliver","Deliver an already-composed unsent preview without rerunning checks. An already requested delivery cannot be replayed with a different key.",json!({"id":{"type":"string"}}),vec!["id"]),
  ("prune","Delete terminal report content and AI runs before UTC millisecond cutoff. AI results owned by active workflows and request tombstones are retained.",json!({"before":{"type":"integer","minimum":0}}),vec!["before"]),
 ] {let read=["list","get","runs","run_get","analysis_get","schedule_get"].contains(&op);let mut props=properties;let mut req=required;if !read{props["requestId"]=json!({"type":"string","minLength":1,"maxLength":64});req.push("requestId");}tools.push(json!({"name":format!("reports_{op}"),"description":description,"inputSchema":{"type":"object","properties":props,"required":req,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":op=="prune","idempotentHint":true,"openWorldHint":!read}}));}
    tools
}
