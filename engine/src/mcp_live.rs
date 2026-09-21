//! Host-routed live commands. Source and VM snapshots are never durable audit payloads.
use crate::{
    authority::{
        ErrorCode as Error, HostGrants, Operation as Op, Permit, Result, Revision, Session, Status,
        Target,
    },
    mcp::Request,
    mcp_engine::AgentEngine,
    runtime::Engine,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};
use tokio::time::Instant;
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Address {
    client_id: String,
    slot_id: String,
    live_instance_id: String,
}
impl Address {
    fn target(&self) -> Target {
        Target::Live {
            client_id: self.client_id.clone(),
            slot_id: self.slot_id.clone(),
            instance_id: self.live_instance_id.clone(),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Command {
    target: Address,
    expected_edit_revision: Option<u64>,
    expected_assignment_revision: Option<u64>,
    #[serde(default)]
    discard_dirty: bool,
    source: Option<String>,
    #[serde(default = "timeout")]
    timeout_ms: u64,
    request_id: Option<String>,
}
fn timeout() -> u64 {
    5000
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Listing {
    client_id: Option<String>,
    cursor: Option<String>,
    #[serde(default = "limit")]
    limit: usize,
}
fn limit() -> usize {
    25
}
struct Pending {
    session: Session,
    connection: String,
    call: String,
    address: Address,
    op: Op,
    permit: Option<Rc<Permit>>,
    args: Value,
    until: Instant,
    claimed: bool,
    begun: bool,
    cancelled: Rc<Cell<bool>>,
    reply: Option<Value>,
    grants: Option<HostGrants>,
    effects: HashSet<u64>,
}
#[derive(Clone)]
pub struct Live {
    engine: Engine,
    agent_engine: AgentEngine,
    pending: Rc<RefCell<HashMap<String, Pending>>>,
    cancelled: Rc<RefCell<Vec<(Session, String, Option<String>, Instant)>>>,
    cancellation_pressure: Rc<Cell<Option<Instant>>>,
}
impl Live {
    pub fn new(engine: Engine, agent_engine: AgentEngine) -> Self {
        Self {
            engine,
            agent_engine,
            pending: Default::default(),
            cancelled: Default::default(),
            cancellation_pressure: Default::default(),
        }
    }
    pub fn cancel(&self, s: &Session, connection: &str, call: Option<&str>) -> Result<()> {
        for p in self.pending.borrow().values() {
            if p.session.same_credential(s)
                && p.connection == connection
                && call.is_none_or(|c| c == p.call)
            {
                p.cancelled.set(true);
            }
        }
        let mut cancelled = self.cancelled.borrow_mut();
        cancelled.retain(|c| c.3.elapsed() < Duration::from_secs(60));
        if cancelled.len() >= 256 {
            // Cancel active work even under pressure; fail closed for new live work.
            self.cancellation_pressure
                .set(Some(Instant::now() + Duration::from_secs(60)));
            return Ok(());
        }
        cancelled.push((
            s.clone(),
            connection.into(),
            call.map(str::to_string),
            Instant::now(),
        ));
        Ok(())
    }
    fn check(&self, p: &Pending) -> Result<()> {
        if p.cancelled.get() {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= p.until {
            return Err(Error::TimedOut);
        }
        let store = self.engine.store.borrow();
        store.agent_require(&p.session, p.op, &p.address.target())?;
        if p.begun {
            if let Some(permit) = &p.permit {
                store.agent_check_permit(permit)?;
            }
        }
        let live = store.client_live_target(
            &p.address.client_id,
            &p.address.slot_id,
            &p.address.live_instance_id,
            p.op == Op::LiveExecute && !p.begun,
            self.engine.now(),
        );
        if !(p.begun && p.op == Op::LiveReload && matches!(live, Err(Error::StaleInstance))) {
            live?;
        }
        Ok(())
    }
    fn revisions(target: &Target, n: u64) -> Vec<Revision> {
        vec![Revision {
            target: target.clone(),
            revision: n.to_string(),
        }]
    }
    pub async fn execute(
        &self,
        s: &Session,
        connection: &str,
        call: &str,
        r: Request,
    ) -> Result<Value> {
        if self
            .cancellation_pressure
            .get()
            .is_some_and(|until| Instant::now() < until)
        {
            return Err(Error::LimitExceeded);
        }
        if connection.len() != 64 || !connection.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput);
        }
        {
            let mut cancelled = self.cancelled.borrow_mut();
            cancelled.retain(|c| c.3.elapsed() < Duration::from_secs(60));
            if cancelled.iter().any(|c| {
                c.0.same_credential(s)
                    && c.1 == connection
                    && c.2.as_ref().is_none_or(|id| id == call)
            }) {
                return Err(Error::Cancelled);
            }
        }
        if r.name == "clients_list" {
            return self.list(s, serde_json::from_value(r.arguments)?);
        }
        let op = match r.name.as_str() {
            "live_inspect" => Op::LiveInspect,
            "live_execute" => Op::LiveExecute,
            "live_reload" => Op::LiveReload,
            _ => return Err(Error::InvalidInput),
        };
        let allowed: &[&str] = match op {
            Op::LiveInspect => &["target"],
            Op::LiveExecute => &[
                "target",
                "expectedEditRevision",
                "source",
                "timeoutMs",
                "requestId",
            ],
            _ => &[
                "target",
                "expectedEditRevision",
                "expectedAssignmentRevision",
                "discardDirty",
                "requestId",
            ],
        };
        if r.arguments
            .as_object()
            .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
        {
            return Err(Error::InvalidInput);
        }
        let args = r.arguments;
        let c: Command = serde_json::from_value(args.clone())?;
        for id in [
            &c.target.client_id,
            &c.target.slot_id,
            &c.target.live_instance_id,
        ] {
            crate::definitions::identifier(id).map_err(|_| Error::InvalidInput)?;
        }
        if !(100..=5000).contains(&c.timeout_ms)
            || c.source.as_ref().is_some_and(|s| s.len() > 65536)
        {
            return Err(Error::InvalidInput);
        }
        if op == Op::LiveExecute && (c.source.is_none() || c.expected_edit_revision.is_none())
            || op == Op::LiveReload
                && (c.expected_edit_revision.is_none() || c.expected_assignment_revision.is_none())
        {
            return Err(Error::InvalidInput);
        }
        if op != Op::LiveExecute && c.source.is_some() {
            return Err(Error::InvalidInput);
        }
        let target = c.target.target();
        let mut permit = None;
        if op != Op::LiveInspect {
            let req = c.request_id.as_deref().ok_or(Error::InvalidInput)?;
            let before = c
                .expected_edit_revision
                .map(|n| Self::revisions(&target, n))
                .unwrap_or_default();
            let admission = self.engine.store.borrow_mut().agent_admit(
                s,
                req,
                op,
                &[target.clone()],
                &args,
                &before,
                self.engine.now(),
            )?;
            if admission.permit.is_none() {
                if admission.record.error == Some(Error::Conflict) {
                    return Ok(json!({"audit":admission.record,"error":"conflict"}));
                }
                return self.outcome(s, req);
            }
            permit = admission.permit.map(Rc::new);
        } else {
            self.engine.store.borrow().agent_require(s, op, &target)?;
        }
        let checked = (|| {
            let store = self.engine.store.borrow();
            let slot = store.client_live_target(
                &c.target.client_id,
                &c.target.slot_id,
                &c.target.live_instance_id,
                op == Op::LiveExecute,
                self.engine.now(),
            )?;
            if c.expected_edit_revision
                .is_some_and(|n| n < slot.report.edit_revision)
            {
                return Err(Error::Conflict);
            }
            if op == Op::LiveReload {
                if slot.report.dirty && !c.discard_dirty {
                    return Err(Error::DirtyAckRequired);
                }
                let a = store.assignment_get(&c.target.client_id, Some(&c.target.slot_id))?;
                if Some(a.revision) != c.expected_assignment_revision {
                    return Err(Error::Conflict);
                }
            }
            if self.pending.borrow().len() >= 64
                || self
                    .pending
                    .borrow()
                    .values()
                    .any(|p| p.address == c.target)
            {
                return Err(Error::LimitExceeded);
            }
            Ok(())
        })();
        if let Err(e) = checked {
            if let Some(p) = permit {
                self.engine.store.borrow_mut().agent_finish(
                    &p,
                    Status::Failed,
                    Some(e),
                    &[],
                    self.engine.now(),
                )?;
            }
            return Err(e);
        }
        let id = crate::mcp_engine::random_id()?;
        self.pending.borrow_mut().insert(
            id.clone(),
            Pending {
                session: s.clone(),
                connection: connection.into(),
                call: call.into(),
                address: c.target,
                op,
                permit: permit.clone(),
                args,
                until: Instant::now() + Duration::from_millis(c.timeout_ms),
                claimed: false,
                begun: false,
                cancelled: Rc::new(Cell::new(false)),
                reply: None,
                grants: None,
                effects: HashSet::new(),
            },
        );
        let result = loop {
            {
                let mut pending = self.pending.borrow_mut();
                let p = pending.get_mut(&id).unwrap();
                if let Some(reply) = p.reply.take() {
                    break Ok(reply);
                }
                if let Err(e) = self.check(p) {
                    break Err(e);
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let p = self.pending.borrow_mut().remove(&id).unwrap();
        if let Some(permit) = permit {
            let (status, error) = match &result {
                Ok(v) if v["error"].is_null() => (Status::Complete, None),
                Ok(v) => (
                    Status::Failed,
                    Some(
                        serde_json::from_value(v["error"].clone()).unwrap_or(Error::InternalError),
                    ),
                ),
                Err(e) => (
                    if p.begun {
                        Status::Unknown
                    } else if *e == Error::Cancelled {
                        Status::Cancelled
                    } else {
                        Status::Failed
                    },
                    Some(*e),
                ),
            };
            let revisions = result
                .as_ref()
                .ok()
                .and_then(|v| v["editRevision"].as_u64())
                .map(|n| Self::revisions(&target, n))
                .unwrap_or_default();
            let mut store = self.engine.store.borrow_mut();
            store.conn.execute_batch("SAVEPOINT live_outcome")?;
            let finish = (|| {
                let audit =
                    store.agent_finish(&permit, status, error, &revisions, self.engine.now())?;
                // Only host lifecycle metadata is persisted, never inspection/state/code/results.
                if let Ok(v) = &result {
                    let metadata = json!({"oldLiveInstanceId":p.address.live_instance_id,"liveInstanceId":v["liveInstanceId"],"packageRevision":v["packageRevision"],"editRevision":v["editRevision"]});
                    store.conn.execute(
                        "INSERT INTO live_outcomes VALUES(?,?)",
                        rusqlite::params![audit.id, metadata.to_string()],
                    )?;
                }
                Ok::<_, Error>(())
            })();
            match finish {
                Ok(()) => store.conn.execute_batch("RELEASE live_outcome")?,
                Err(e) => {
                    store
                        .conn
                        .execute_batch("ROLLBACK TO live_outcome; RELEASE live_outcome")?;
                    return Err(e);
                }
            }
            drop(store);
            self.outcome(s, c.request_id.as_deref().unwrap())
        } else {
            result
        }
    }
    pub fn outcome(&self, s: &Session, id: &str) -> Result<Value> {
        use rusqlite::OptionalExtension;
        let store = self.engine.store.borrow();
        let audit = store.agent_status(s, id)?;
        let saved: Option<String> = store
            .conn
            .query_row(
                "SELECT body FROM live_outcomes WHERE audit=?",
                [audit.id],
                |r| r.get(0),
            )
            .optional()?;
        let mut out = json!({"audit":audit});
        if let Some(saved) = saved {
            out["outcome"] = serde_json::from_str(&saved)?;
        }
        if let Some(e) = audit.error {
            out["error"] = json!(e);
        }
        Ok(out)
    }
    fn list(&self, s: &Session, r: Listing) -> Result<Value> {
        if !(1..=50).contains(&r.limit) {
            return Err(Error::InvalidInput);
        }
        let store = self.engine.store.borrow();
        let context = json!(["clients", r.client_id]);
        let offset = r
            .cursor
            .as_deref()
            .map(|c| store.agent_cursor_read(s, &context, c))
            .transpose()?
            .unwrap_or(0) as usize;
        let mut all = vec![];
        for mut c in store.clients_list(self.engine.now())? {
            let id = c["clientId"].as_str().unwrap().to_string();
            if r.client_id.as_ref().is_some_and(|x| x != &id) {
                continue;
            }
            let client_allowed = store
                .agent_require(
                    s,
                    Op::ClientsList,
                    &Target::Client {
                        client_id: id.clone(),
                    },
                )
                .is_ok();
            let slots = c["slots"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|v| {
                    store
                        .agent_require(
                            s,
                            Op::ClientsList,
                            &Target::Live {
                                client_id: id.clone(),
                                slot_id: v["slotId"].as_str().unwrap().into(),
                                instance_id: v["liveInstanceId"].as_str().unwrap().into(),
                            },
                        )
                        .is_ok()
                })
                .cloned()
                .collect::<Vec<_>>();
            if !client_allowed && slots.is_empty() {
                continue;
            }
            c["slots"] = json!(slots);
            if !client_allowed {
                c.as_object_mut().unwrap().remove("defaultAssignment");
            }
            all.push(c);
        }
        if offset > all.len() {
            return Err(Error::InvalidInput);
        }
        let end = (offset + r.limit).min(all.len());
        let next = if end < all.len() {
            Some(store.agent_cursor(s, &context, end as u64)?)
        } else {
            None
        };
        Ok(json!({"clients":all[offset..end],"nextCursor":next}))
    }
    pub async fn host(&self, credential: &str, body: Value) -> Result<Value> {
        let r: HostRequest = serde_json::from_value(body)?;
        let store = self.engine.store.borrow();
        let host = store.client_authenticate(credential)?;
        let slot = store.client_slot(&host, &r.slot, &r.owner)?;
        if r.op != "liveFinish" && (slot.live_instance_id != r.live || slot.epoch != r.epoch) {
            return Err(Error::StaleInstance);
        }
        drop(store);
        if r.op == "livePoll" {
            let mut all = self.pending.borrow_mut();
            let mut commands = vec![];
            for (id, p) in all.iter_mut() {
                if p.address.client_id == host.id()
                    && p.address.slot_id == r.slot
                    && p.address.live_instance_id == r.live
                    && !p.claimed
                    && self.check(p).is_ok()
                {
                    p.claimed = true;
                    commands.push(json!({"id":id,"operation":p.op,"arguments":p.args,"remainingMs":p.until.saturating_duration_since(Instant::now()).as_millis()}));
                }
            }
            return Ok(json!({"commands":commands}));
        }
        let id = r.command_id.as_deref().ok_or(Error::InvalidInput)?;
        // Extract shared capability before any asynchronous engine operation.
        let (session, permit, grants, flag, until) = {
            let mut all = self.pending.borrow_mut();
            let p = all.get_mut(id).ok_or(Error::Cancelled)?;
            if p.address.client_id != host.id()
                || p.address.slot_id != r.slot
                || p.address.live_instance_id != r.live
            {
                return Err(Error::Forbidden);
            }
            if r.op == "liveFinish" {
                if p.cancelled.get() || Instant::now() >= p.until {
                    return Err(Error::Cancelled);
                }
                self.engine
                    .store
                    .borrow()
                    .agent_require(&p.session, p.op, &p.address.target())?;
            } else {
                self.check(p)?;
            }
            if r.op == "liveBegin" {
                if !p.claimed || p.begun {
                    return Err(Error::Conflict);
                }
                let report = r.report.as_ref().ok_or(Error::InvalidInput)?;
                if report.live_instance_id != r.live
                    || report.edit_revision
                        != p.args["expectedEditRevision"]
                            .as_u64()
                            .unwrap_or(report.edit_revision)
                {
                    return Err(Error::Conflict);
                }
                if !report.foreground
                    || report.lifecycle == "paused"
                    || p.op == Op::LiveExecute && report.lifecycle == "failed"
                {
                    return Err(Error::TargetUnavailable);
                }
                if p.op == Op::LiveReload
                    && report.dirty
                    && !p.args["discardDirty"].as_bool().unwrap_or(false)
                {
                    return Err(Error::DirtyAckRequired);
                }
                let mut store = self.engine.store.borrow_mut();
                let delivery = if p.op == Op::LiveReload {
                    let d = store.delivery_prepare(&host, &r.slot, &r.owner)?;
                    if Some(d.assignment.revision) != p.args["expectedAssignmentRevision"].as_u64()
                    {
                        return Err(Error::Conflict);
                    }
                    Some(d)
                } else {
                    None
                };
                // Host sends grants from its immutable loaded package; never from the live guest.
                p.grants = r.grants.clone();
                if p.op == Op::LiveExecute && p.grants.is_none() {
                    return Err(Error::InvalidInput);
                }
                if let Some(permit) = &p.permit {
                    store.agent_start(permit, self.engine.now())?;
                }
                p.begun = true;
                return Ok(
                    json!({"delivery":delivery,"remainingMs":p.until.saturating_duration_since(Instant::now()).as_millis()}),
                );
            }
            if r.op == "liveFinish" {
                let value = r.value.ok_or(Error::InvalidInput)?;
                if !p.begun && value["error"].is_null() {
                    return Err(Error::Conflict);
                }
                if serde_json::to_vec(&value)?.len() > 262144 {
                    return Err(Error::LimitExceeded);
                }
                p.reply = Some(value);
                return Ok(json!({"accepted":true}));
            }
            if !p.begun {
                return Err(Error::Conflict);
            }
            if p.op == Op::LiveExecute {
                self.engine.store.borrow().client_live_target(
                    &p.address.client_id,
                    &p.address.slot_id,
                    &p.address.live_instance_id,
                    true,
                    self.engine.now(),
                )?;
            }
            if r.op == "liveCheck" {
                if p.op == Op::LiveReload {
                    self.engine.store.borrow().delivery_confirm(
                        &host,
                        &r.slot,
                        &r.owner,
                        p.args["expectedAssignmentRevision"]
                            .as_u64()
                            .ok_or(Error::InvalidInput)?,
                    )?;
                }
                return Ok(json!({"active":true}));
            }
            if r.op != "liveEffect" || p.op != Op::LiveExecute {
                return Err(Error::InvalidInput);
            }
            let effect = r.effect.as_ref().ok_or(Error::InvalidInput)?;
            if effect.sequence == 0 || p.effects.len() >= 128 {
                return Err(Error::LimitExceeded);
            }
            if !p.effects.insert(effect.sequence) {
                return Err(Error::Conflict);
            }
            (
                p.session.clone(),
                p.permit.clone().ok_or(Error::Forbidden)?,
                p.grants.clone().ok_or(Error::Forbidden)?,
                p.cancelled.clone(),
                p.until,
            )
        };
        let effect = r.effect.ok_or(Error::InvalidInput)?;
        let resource = effect.id.clone();
        let op = match effect.op.as_str() {
            "read" => Op::EngineRead,
            "write" => Op::EngineWrite,
            "run" => Op::EngineRun,
            _ => return Err(Error::Forbidden),
        };
        self.engine
            .store
            .borrow()
            .agent_live_effect(&permit, op, &resource, &grants)?;
        let value = match op {
            Op::EngineRead => {
                if !resource.starts_with("monitor.") {
                    self.engine
                        .read(&resource, Duration::from_millis(5000))
                        .await
                        .map_err(|_| Error::ValidationFailed)?;
                }
                if flag.get() || Instant::now() >= until {
                    return Err(Error::Cancelled);
                }
                self.engine
                    .store
                    .borrow()
                    .agent_live_effect(&permit, op, &resource, &grants)?;
                {
                    let all = self.pending.borrow();
                    self.check(all.get(id).ok_or(Error::Cancelled)?)?;
                }
                self.agent_engine.current(&resource)?
            }
            Op::EngineWrite => {
                let expected = self
                    .engine
                    .store
                    .borrow()
                    .instance(&resource)
                    .map_err(|_| Error::NotFound)?
                    .revision;
                let value = effect.value.ok_or(Error::InvalidInput)?;
                crate::value::validate(&value).map_err(|_| Error::InvalidInput)?;
                self.engine
                    .write(&resource, expected, value)
                    .map_err(|_| Error::ValidationFailed)?;
                self.agent_engine.current(&resource)?
            }
            Op::EngineRun => {
                let a=self.agent_engine.execute(&session,id,&crate::mcp_engine::random_id()?,Request{name:"engine_run".into(),arguments:json!({"id":resource,"requestId":format!("live-{}-{}",id,effect.sequence)})}).await?;
                a
            }
            _ => return Err(Error::Forbidden),
        };
        Ok(value)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostRequest {
    op: String,
    slot: String,
    owner: String,
    live: String,
    epoch: u64,
    command_id: Option<String>,
    report: Option<LocalReport>,
    grants: Option<HostGrants>,
    value: Option<Value>,
    effect: Option<Effect>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalReport {
    live_instance_id: String,
    edit_revision: u64,
    foreground: bool,
    lifecycle: String,
    dirty: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Effect {
    op: String,
    id: String,
    value: Option<Value>,
    sequence: u64,
}
pub fn tools() -> Vec<Value> {
    let target = json!({"type":"object","additionalProperties":false,"properties":{"clientId":{"type":"string"},"slotId":{"type":"string"},"liveInstanceId":{"type":"string"}},"required":["clientId","slotId","liveInstanceId"]});
    let mut out = vec![];
    for (name, props, required) in [
        (
            "clients_list",
            json!({"clientId":{"type":"string"},"cursor":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":50}}),
            vec![],
        ),
        ("live_inspect", json!({"target":target}), vec!["target"]),
        (
            "live_execute",
            json!({"target":target,"source":{"type":"string","maxLength":65536},"expectedEditRevision":{"type":"integer","minimum":0},"timeoutMs":{"type":"integer","minimum":100,"maximum":5000},"requestId":{"type":"string"}}),
            vec!["target", "source", "expectedEditRevision", "requestId"],
        ),
        (
            "live_reload",
            json!({"target":target,"expectedEditRevision":{"type":"integer","minimum":0},"expectedAssignmentRevision":{"type":"integer","minimum":1},"discardDirty":{"type":"boolean"},"requestId":{"type":"string"}}),
            vec![
                "target",
                "expectedEditRevision",
                "expectedAssignmentRevision",
                "requestId",
            ],
        ),
    ] {
        out.push(json!({"name":name,"description":match name{"clients_list"=>"List authorized client registrations and timestamped live slots.","live_inspect"=>"Read the exact foreground live VM snapshot without executing expressions or marking dirty.","live_execute"=>"Execute an async JavaScript function body with ctx.state(), await ctx.commit(snapshot,next), ctx.read(id), ctx.write(id,value), ctx.run(id). Temporary state edits mark dirty; UI and saved sources remain immutable. No implicit retry.",_=>"Reload the exact foreground instance from a pinned saved package. Dirty instances require discardDirty:true and current revisions. Earlier server effects survive."},"inputSchema":{"type":"object","additionalProperties":false,"properties":props,"required":required},"annotations":{"readOnlyHint":name=="clients_list"||name=="live_inspect"}}));
    }
    out
}

#[cfg(test)]
mod tests;
