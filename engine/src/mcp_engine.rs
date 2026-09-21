//! Authenticated engine tools. Leases belong to a principal, credential and adapter session.
use crate::{
    authority::{
        AuditRecord, ErrorCode as Error, Operation as Op, Result, Revision, Session, Status, Target,
    },
    definitions::identifier,
    mcp::Request,
    pipelines::{Pipelines, Run},
    runtime::Engine,
    store::{Instance, Sample},
    value,
    watches::Watches,
};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};
use tokio::time::Instant;
const LEASE: Duration = Duration::from_secs(60);
#[derive(Clone)]
pub struct AgentEngine {
    pub engine: Engine,
    pipelines: Pipelines,
    watches: Watches,
    connections: Rc<RefCell<HashMap<String, Connection>>>,
}
struct Connection {
    session: Session,
    last: Instant,
    closed: bool,
    calls: HashMap<String, Rc<Cell<bool>>>,
    cancelled: HashSet<String>,
    subscriptions: HashMap<String, Subscription>,
}
struct Subscription {
    ids: Vec<String>,
    seen: Vec<Value>,
    last: Instant,
}
struct CallGuard {
    connections: Rc<RefCell<HashMap<String, Connection>>>,
    connection: String,
    call: String,
}
impl Drop for CallGuard {
    fn drop(&mut self) {
        if let Some(c) = self.connections.borrow_mut().get_mut(&self.connection) {
            c.calls.remove(&self.call);
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    id: String,
    #[serde(default = "timeout")]
    timeout_ms: u64,
}
fn timeout() -> u64 {
    5000
}
fn limit() -> usize {
    50
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    id: String,
    cursor: Option<String>,
    #[serde(default = "limit")]
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Subscribe {
    ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubscriptionId {
    subscription_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Write {
    id: String,
    expected_revision: u64,
    value: Value,
    request_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Action {
    id: String,
    request_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunId {
    run_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cancel {
    run_id: String,
    request_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelCall {
    call_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusRequest {
    request_id: String,
}
fn target(id: &str) -> Target {
    Target::Resource { id: id.into() }
}
fn valid(id: &str) -> Result<()> {
    identifier(id).map_err(|_| Error::InvalidInput)
}
fn code(e: String) -> Error {
    match e.as_str() {
        "conflict" => Error::Conflict,
        "cancelled" | "agent_cancelled" => Error::Cancelled,
        "agent_forbidden" => Error::Forbidden,
        "agent_unauthenticated" => Error::Unauthenticated,
        "read timeout" | "setter timeout" | "evaluation timeout" => Error::TimedOut,
        "evaluation capacity" | "run capacity" => Error::LimitExceeded,
        "Pipeline disabled/unavailable" => Error::TargetUnavailable,
        "not a Watch" | "use computed setter" => Error::InvalidInput,
        "Query returned no rows" => Error::NotFound,
        _ => Error::ValidationFailed,
    }
}
fn guard_error(e: Error) -> String {
    match e {
        Error::Unauthenticated => "agent_unauthenticated",
        Error::Forbidden => "agent_forbidden",
        _ => "agent_cancelled",
    }
    .into()
}
fn sample(i: &Instance) -> Value {
    json!({"id":i.id,"value":i.value,"hasValue":i.has_value,"timestamp":i.timestamp,"quality":i.quality,"revision":i.revision,"generation":i.generation})
}
fn safe_run(r: &Run) -> Value {
    json!({"id":r.id,"instance":r.instance,"generation":r.generation,"status":r.status,"cause":r.cause,"created":r.created,"started":r.started,"finished":r.finished,"attempt":r.attempt,"error":r.error.as_ref().map(|_|match r.status.as_str(){"cancelled"=>"cancelled","timed_out"=>"timed_out","unknown"=>"unknown",_=>"execution_failed"}),"result":r.result,"effectPending":r.effect_pending})
}
fn output(audit: AuditRecord) -> Value {
    let error = audit.error;
    let mut out = json!({"audit":audit});
    if let Some(error) = error {
        out["error"] = json!(error);
    }
    out
}
pub fn random_id() -> Result<String> {
    use ring::rand::SecureRandom;
    let mut bytes = [0; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::InternalError)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
impl AgentEngine {
    pub fn new(engine: Engine, pipelines: Pipelines, watches: Watches) -> Self {
        Self {
            engine,
            pipelines,
            watches,
            connections: Default::default(),
        }
    }
    fn require(&self, s: &Session, op: Op, id: &str) -> Result<()> {
        self.engine.store.borrow().agent_require(s, op, &target(id))
    }
    fn connection(&self, s: &Session, id: &str) -> Result<()> {
        if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput);
        }
        self.sweep();
        let mut all = self.connections.borrow_mut();
        if let Some(c) = all.get_mut(id) {
            if !c.session.same_credential(s) {
                return Err(Error::Forbidden);
            }
            if c.closed {
                return Err(Error::Cancelled);
            }
            c.last = Instant::now();
            return Ok(());
        }
        if all.len() >= 128 {
            return Err(Error::LimitExceeded);
        }
        all.insert(
            id.into(),
            Connection {
                session: s.clone(),
                last: Instant::now(),
                closed: false,
                calls: HashMap::new(),
                cancelled: HashSet::new(),
                subscriptions: HashMap::new(),
            },
        );
        Ok(())
    }
    fn release(&self, sub: Subscription) {
        for id in sub.ids {
            if !id.starts_with("monitor.") {
                self.engine.unsubscribe(&id);
            }
        }
    }
    pub fn sweep(&self) {
        let mut all = self.connections.borrow_mut();
        let mut remove = vec![];
        for (id, c) in all.iter_mut() {
            let expired = c.last.elapsed() >= LEASE;
            let subs: Vec<_> =
                c.subscriptions
                    .iter()
                    .filter(|(_, sub)| {
                        expired
                            || sub.last.elapsed() >= LEASE
                            || sub.ids.iter().any(|id| {
                                self.require(&c.session, Op::EngineSubscribe, id).is_err()
                            })
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
            for id in subs {
                if let Some(sub) = c.subscriptions.remove(&id) {
                    self.release(sub);
                }
            }
            if expired {
                for flag in c.calls.values() {
                    flag.set(false);
                }
                if c.calls.is_empty() {
                    remove.push(id.clone());
                }
            }
        }
        for id in remove {
            all.remove(&id);
        }
    }
    pub fn close(&self, s: &Session, connection: &str) -> Result<()> {
        self.connection(s, connection)?;
        let mut all = self.connections.borrow_mut();
        let c = all.get_mut(connection).unwrap();
        c.closed = true;
        for flag in c.calls.values() {
            flag.set(false);
        }
        for (_, sub) in c.subscriptions.drain() {
            self.release(sub);
        }
        Ok(())
    }
    fn current(&self, id: &str) -> Result<Value> {
        if let Some(monitor) = id.strip_prefix("monitor.") {
            let store = self.engine.store.borrow();
            let state = store.monitor_state(monitor).map_err(code)?;
            let latest_id: Option<String> = store
                .conn
                .query_row(
                    "SELECT id FROM monitoring_runs WHERE instance=? ORDER BY rowid DESC LIMIT 1",
                    [monitor],
                    |r| r.get(0),
                )
                .optional()?;
            let latest = latest_id
                .as_deref()
                .map(|id| store.run(id).map_err(code))
                .transpose()?;
            // Watch state is the existing published monitoring resource. Exceptions remain private.
            let mut payload = value::from_json(
                &json!({"state":null,"faulted":state.faulted,"error":state.error.as_ref().map(|_|"execution_failed"),"nextDue":state.next_due,"missed":state.missed,"lastGap":state.last_gap,"lastActions":state.last_actions,"run":latest.as_ref().map(safe_run)}),
            );
            for pair in payload["value"][1].as_array_mut().unwrap() {
                if pair[0] == "state" {
                    pair[1] = state.state["value"].clone();
                }
            }
            return Ok(
                json!({"id":id,"value":payload,"hasValue":true,"revision":store.revision().map_err(code)?,"generation":state.generation,"timestamp":latest.as_ref().and_then(|r|r.finished.or(r.started)).unwrap_or(0),"quality":if state.error.is_some(){"error"}else{"good"},"evaluation":if state.faulted{"error"}else{"idle"},"error":state.error.as_ref().map(|_|"execution_failed")}),
            );
        }
        let i = self.engine.store.borrow().instance(id).map_err(code)?;
        let mut v = sample(&i);
        let all = self.engine.evaluation.borrow();
        let meta = all.get(id);
        v["evaluation"] = json!(if meta.is_some_and(|m| m.running > 0) {
            "refreshing"
        } else if meta.is_some_and(|m| m.error.is_some()) {
            "error"
        } else if meta.is_some_and(|m| m.invalidated) {
            "invalidated"
        } else {
            "idle"
        });
        v["error"] = json!(meta
            .and_then(|m| m.error.as_ref())
            .map(|_| "evaluation_failed"));
        Ok(v)
    }
    pub fn status(&self, s: &Session, request_id: &str) -> Result<Value> {
        let store = self.engine.store.borrow();
        let audit = store.agent_status(s, request_id)?;
        let admission = if audit.operation == Op::EngineRun && audit.status == Status::Complete {
            store
                .run_admission(&format!("agent-{}", audit.id))
                .map_err(code)?
        } else {
            None
        };
        let mut out = json!({"audit":audit});
        if let Some(admission) = admission {
            out["admission"] = json!(admission);
        }
        Ok(out)
    }
    pub async fn execute(
        &self,
        s: &Session,
        connection: &str,
        call: &str,
        r: Request,
    ) -> Result<Value> {
        self.connection(s, connection)?;
        if r.name == "_session_close" {
            if r.arguments != json!({}) {
                return Err(Error::InvalidInput);
            }
            self.close(s, connection)?;
            return Ok(json!({"closed":true}));
        }
        if r.name == "_cancel_call" {
            let r: CancelCall = serde_json::from_value(r.arguments)?;
            valid(&r.call_id)?;
            let mut all = self.connections.borrow_mut();
            let c = all.get_mut(connection).unwrap();
            if let Some(flag) = c.calls.get(&r.call_id) {
                flag.set(false);
            } else if c.cancelled.len() < 128 {
                c.cancelled.insert(r.call_id);
            } else {
                return Err(Error::LimitExceeded);
            }
            return Ok(json!({"cancelled":true}));
        }
        valid(call)?;
        let flag = Rc::new(Cell::new(true));
        {
            let mut all = self.connections.borrow_mut();
            let c = all.get_mut(connection).unwrap();
            if c.cancelled.remove(call) {
                return Err(Error::Cancelled);
            }
            if c.calls.len() >= 16 || c.calls.contains_key(call) {
                return Err(Error::LimitExceeded);
            }
            c.calls.insert(call.into(), flag.clone());
        }
        let _guard = CallGuard {
            connections: self.connections.clone(),
            connection: connection.into(),
            call: call.into(),
        };
        let a = r.arguments;
        match r.name.as_str() {
            "operation_status" => {
                let r: StatusRequest = serde_json::from_value(a)?;
                self.status(s, &r.request_id)
            }
            "engine_read" => {
                let r: Read = serde_json::from_value(a)?;
                if !(1..=5000).contains(&r.timeout_ms) {
                    return Err(Error::InvalidInput);
                }
                self.require(s, Op::EngineRead, &r.id)?;
                if !r.id.starts_with("monitor.") {
                    self.engine
                        .read(&r.id, Duration::from_millis(r.timeout_ms))
                        .await
                        .map_err(code)?;
                }
                if !flag.get() {
                    return Err(Error::Cancelled);
                }
                self.require(s, Op::EngineRead, &r.id)?;
                Ok(json!({"sample":self.current(&r.id)?}))
            }
            "engine_history" => {
                let r: History = serde_json::from_value(a)?;
                self.require(s, Op::EngineHistory, &r.id)?;
                self.history(s, &r)
            }
            "engine_subscribe" => {
                let r: Subscribe = serde_json::from_value(a)?;
                self.subscribe(s, connection, r.ids)
            }
            "engine_poll" => {
                let r: SubscriptionId = serde_json::from_value(a)?;
                self.poll(s, connection, &r.subscription_id)
            }
            "engine_unsubscribe" => {
                let r: SubscriptionId = serde_json::from_value(a)?;
                valid(&r.subscription_id)?;
                let sub = self
                    .connections
                    .borrow_mut()
                    .get_mut(connection)
                    .unwrap()
                    .subscriptions
                    .remove(&r.subscription_id)
                    .ok_or(Error::NotFound)?;
                self.release(sub);
                Ok(json!({"released":true}))
            }
            "engine_write" | "engine_set" => {
                let set = r.name == "engine_set";
                let r: Write = serde_json::from_value(a.clone())?;
                self.mutate_value(s, &r, a, set, flag).await
            }
            "engine_run" | "engine_resume_watch" => {
                let action: Action = serde_json::from_value(a.clone())?;
                let op = if r.name == "engine_run" {
                    Op::EngineRun
                } else {
                    Op::EngineResumeWatch
                };
                self.action(s, op, &action.id, &action.request_id, a, None)
            }
            "engine_run_status" => {
                let r: RunId = serde_json::from_value(a)?;
                valid(&r.run_id)?;
                let run = self.engine.store.borrow().run(&r.run_id).map_err(code)?;
                self.require(s, Op::EngineRunStatus, &run.instance)?;
                Ok(json!({"run":safe_run(&run)}))
            }
            "engine_cancel_run" => {
                let r: Cancel = serde_json::from_value(a.clone())?;
                valid(&r.run_id)?;
                let run = self.engine.store.borrow().run(&r.run_id).map_err(code)?;
                self.action(
                    s,
                    Op::EngineCancelRun,
                    &run.instance,
                    &r.request_id,
                    a,
                    Some(&r.run_id),
                )
            }
            _ => Err(Error::InvalidInput),
        }
    }
    fn subscribe(&self, s: &Session, connection: &str, ids: Vec<String>) -> Result<Value> {
        if ids.is_empty() || ids.len() > 8 || ids.iter().collect::<HashSet<_>>().len() != ids.len()
        {
            return Err(Error::InvalidInput);
        }
        {
            let all = self.connections.borrow();
            if all.values().map(|c| c.subscriptions.len()).sum::<usize>() >= 128
                || all[connection].subscriptions.len() >= 16
            {
                return Err(Error::LimitExceeded);
            }
        }
        let mut initial = vec![];
        for id in &ids {
            self.require(s, Op::EngineSubscribe, id)?;
            initial.push(self.current(id)?);
        }
        let id = random_id()?;
        let mut added: Vec<String> = vec![];
        for resource in &ids {
            if !resource.starts_with("monitor.") {
                if let Err(e) = self.engine.subscribe(resource) {
                    for id in added {
                        self.engine.unsubscribe(&id);
                    }
                    return Err(code(e));
                }
                added.push(resource.clone());
            }
        }
        for resource in &added {
            let computed = {
                let store = self.engine.store.borrow();
                store
                    .instance(resource)
                    .and_then(|i| store.definition(&i.definition))
                    .is_ok_and(|d| d.kind == "computed")
            };
            if computed {
                self.engine.catalog_changed(resource);
            }
        }
        self.connections
            .borrow_mut()
            .get_mut(connection)
            .unwrap()
            .subscriptions
            .insert(
                id.clone(),
                Subscription {
                    ids,
                    seen: initial.clone(),
                    last: Instant::now(),
                },
            );
        Ok(json!({"subscriptionId":id,"values":initial,"leaseMs":60000}))
    }
    fn poll(&self, s: &Session, connection: &str, id: &str) -> Result<Value> {
        valid(id)?;
        let mut all = self.connections.borrow_mut();
        let c = all.get_mut(connection).unwrap();
        let sub = c.subscriptions.get_mut(id).ok_or(Error::NotFound)?;
        let mut current = vec![];
        for resource in &sub.ids {
            self.require(s, Op::EngineSubscribe, resource)?;
            current.push(match self.current(resource) {
                Ok(v) => v,
                Err(Error::NotFound) => json!({"id":resource,"error":"not_found"}),
                Err(e) => return Err(e),
            });
        }
        let values: Vec<_> = current
            .iter()
            .zip(&sub.seen)
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a.clone())
            .collect();
        sub.seen = current;
        sub.last = Instant::now();
        Ok(json!({"subscriptionId":id,"values":values,"leaseMs":60000}))
    }
    fn history(&self, s: &Session, r: &History) -> Result<Value> {
        if !(1..=100).contains(&r.limit) {
            return Err(Error::InvalidInput);
        }
        let store = self.engine.store.borrow();
        let i = store.instance(&r.id).map_err(code)?;
        let context = json!(["engine_history", r.id, i.generation]);
        let before = r
            .cursor
            .as_deref()
            .map(|c| store.agent_cursor_read(s, &context, c))
            .transpose()?
            .unwrap_or(i64::MAX as u64);
        let mut q=store.conn.prepare("SELECT seq,body FROM history WHERE instance=? AND seq<? AND timestamp>=? AND seq IN (SELECT seq FROM history WHERE instance=? ORDER BY seq DESC LIMIT ?) ORDER BY seq DESC LIMIT ?")?;
        let rows = q.query_map(
            rusqlite::params![
                r.id,
                before,
                self.engine.now().saturating_sub(i.history_age_ms),
                r.id,
                i.history_count,
                r.limit + 1
            ],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
        )?;
        let mut samples = vec![];
        let mut last = before;
        let mut size = 0;
        let mut more = false;
        for row in rows {
            let (seq, body) = row?;
            if samples.len() == r.limit || size + body.len() > 1_048_576 {
                more = true;
                break;
            }
            size += body.len();
            let sample: Sample = serde_json::from_str(&body)?;
            samples.push(sample);
            last = seq;
        }
        let next = if more {
            Some(store.agent_cursor(s, &context, last)?)
        } else {
            None
        };
        Ok(json!({"id":r.id,"samples":samples,"nextCursor":next}))
    }
    async fn mutate_value(
        &self,
        s: &Session,
        r: &Write,
        args: Value,
        set: bool,
        flag: Rc<Cell<bool>>,
    ) -> Result<Value> {
        valid(&r.id)?;
        value::validate(&r.value).map_err(|_| Error::InvalidInput)?;
        let op = if set { Op::EngineSet } else { Op::EngineWrite };
        let resource = target(&r.id);
        let before = self
            .engine
            .store
            .borrow()
            .instance(&r.id)
            .ok()
            .map(|i| Revision {
                target: resource.clone(),
                revision: i.revision.to_string(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let admission = self.engine.store.borrow_mut().agent_admit(
            s,
            &r.request_id,
            op,
            &[resource.clone()],
            &args,
            &before,
            self.engine.now(),
        )?;
        let Some(permit) = admission.permit else {
            let mut out = output(admission.record);
            if out["error"] == "conflict" && self.require(s, op, &r.id).is_ok() {
                if let Ok(i) = self.engine.store.borrow().instance(&r.id) {
                    out["currentRevision"] = json!(i.revision);
                }
            }
            return Ok(out);
        };
        self.engine
            .store
            .borrow_mut()
            .agent_start(&permit, self.engine.now())?;
        let permit = Rc::new(permit);
        let check = (|| {
            self.engine.store.borrow().agent_check_permit(&permit)?;
            let store = self.engine.store.borrow();
            let i = store.instance(&r.id).map_err(code)?;
            let d = store.definition(&i.definition).map_err(code)?;
            if i.revision != r.expected_revision {
                return Err(Error::Conflict);
            }
            if (d.kind == "computed") != set {
                return Err(Error::InvalidInput);
            }
            Ok(())
        })();
        let result = match check {
            Err(e) => Err(e),
            Ok(()) if !flag.get() => Err(Error::Cancelled),
            Ok(()) if set => {
                let engine = self.engine.clone();
                let p = permit.clone();
                let session = s.clone();
                let guard = Rc::new(
                    move |operation: &str, id: &str| -> crate::store::Result<()> {
                        if !flag.get() {
                            return Err("agent_cancelled".into());
                        }
                        let store = engine.store.borrow();
                        store.agent_check_permit(&p).map_err(guard_error)?;
                        let op = match operation {
                            "read" => Op::EngineRead,
                            "write" => Op::EngineWrite,
                            _ => Op::EngineSet,
                        };
                        store
                            .agent_require(&session, op, &target(id))
                            .map_err(guard_error)?;
                        Ok(())
                    },
                );
                self.engine
                    .set_guarded(&r.id, r.value.clone(), guard)
                    .await
                    .map_err(code)
            }
            Ok(()) => self
                .engine
                .write(&r.id, r.expected_revision, r.value.clone())
                .map_err(code),
        };
        let after = self
            .engine
            .store
            .borrow()
            .instance(&r.id)
            .ok()
            .map(|i| Revision {
                target: resource,
                revision: i.revision.to_string(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let (status, error) = match result {
            Ok(_) => (Status::Complete, None),
            Err(Error::Cancelled) => (Status::Cancelled, Some(Error::Cancelled)),
            Err(Error::TimedOut) => (Status::TimedOut, Some(Error::TimedOut)),
            Err(e) => (Status::Failed, Some(e)),
        };
        let record = self.engine.store.borrow_mut().agent_finish(
            &permit,
            status,
            error,
            &after,
            self.engine.now(),
        )?;
        self.require(s, op, &r.id)?;
        let mut out = output(record);
        if error == Some(Error::Conflict) {
            out["currentRevision"] = json!(
                self.engine
                    .store
                    .borrow()
                    .instance(&r.id)
                    .map_err(code)?
                    .revision
            );
        }
        Ok(out)
    }
    fn action(
        &self,
        s: &Session,
        op: Op,
        id: &str,
        request_id: &str,
        args: Value,
        run_id: Option<&str>,
    ) -> Result<Value> {
        valid(id)?;
        let before = self
            .engine
            .store
            .borrow()
            .monitor_state(id)
            .ok()
            .map(|m| Revision {
                target: target(id),
                revision: m.revision.to_string(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let admission = self.engine.store.borrow_mut().agent_admit(
            s,
            request_id,
            op,
            &[target(id)],
            &args,
            &before,
            self.engine.now(),
        )?;
        let Some(permit) = admission.permit else {
            let mut out = output(admission.record.clone());
            if admission.record.status == Status::Complete && op == Op::EngineRun {
                out["admission"] = json!(self
                    .engine
                    .store
                    .borrow()
                    .run_admission(&format!("agent-{}", admission.record.id))
                    .map_err(code)?);
            }
            return Ok(out);
        };
        let started = self
            .engine
            .store
            .borrow_mut()
            .agent_start(&permit, self.engine.now())?;
        self.engine.store.borrow().agent_check_permit(&permit)?;
        if op == Op::EngineRun {
            // Pipeline admission and its audit commit together. The server owns execution afterwards.
            let mut store = self.engine.store.borrow_mut();
            store.conn.execute_batch("SAVEPOINT agent_run")?;
            let result = (|| {
                let admission = store
                    .admit(
                        id,
                        &format!("agent-{}", started.id),
                        "manual",
                        0,
                        self.engine.now(),
                    )
                    .map_err(code)?;
                let audit =
                    store.agent_finish(&permit, Status::Complete, None, &[], self.engine.now())?;
                Ok(json!({"audit":audit,"admission":admission}))
            })();
            return match result {
                Ok(v) => {
                    store.conn.execute_batch("RELEASE agent_run")?;
                    drop(store);
                    let _ = self.pipelines.pump();
                    Ok(v)
                }
                Err(error) => {
                    store
                        .conn
                        .execute_batch("ROLLBACK TO agent_run; RELEASE agent_run")?;
                    Ok(output(store.agent_finish(
                        &permit,
                        Status::Failed,
                        Some(error),
                        &[],
                        self.engine.now(),
                    )?))
                }
            };
        }
        let result = match op {
            Op::EngineCancelRun => self
                .pipelines
                .cancel(run_id.ok_or(Error::InvalidInput)?)
                .map(|_| ())
                .map_err(code),
            Op::EngineResumeWatch => self.watches.resume(id).map_err(code),
            _ => Err(Error::InvalidInput),
        };
        let after = self
            .engine
            .store
            .borrow()
            .monitor_state(id)
            .ok()
            .map(|m| Revision {
                target: target(id),
                revision: m.revision.to_string(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let error = result.err();
        let audit = self.engine.store.borrow_mut().agent_finish(
            &permit,
            if error.is_none() {
                Status::Complete
            } else {
                Status::Failed
            },
            error,
            &after,
            self.engine.now(),
        )?;
        Ok(output(audit))
    }
}

pub fn tools() -> Vec<Value> {
    fn obj(properties: Value, required: &[&str]) -> Value {
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
    let id = json!({"type":"string","minLength":1,"maxLength":128});
    let value = obj(
        json!({"version":{"const":1},"value":{"type":"array","minItems":1}}),
        &["version", "value"],
    );
    let write = obj(
        json!({"id":id,"expectedRevision":{"type":"integer","minimum":0},"value":value,"requestId":id}),
        &["id", "expectedRevision", "value", "requestId"],
    );
    let action = obj(json!({"id":id,"requestId":id}), &["id", "requestId"]);
    let subscription = obj(json!({"subscriptionId":id}), &["subscriptionId"]);
    [
 ("engine_read","Read a Variable or monitor.ID sample. Computed reads await the shared server producer; timeout stops only this wait. Returns tagged values and public metadata, never private Variable state.",obj(json!({"id":id,"timeoutMs":{"type":"integer","minimum":1,"maximum":5000,"default":5000}}),&["id"]),true),
 ("engine_history","Read bounded newest-first retained Variable samples. Optional opaque cursor continues the same resource generation; retention may remove samples between pages.",obj(json!({"id":id,"limit":{"type":"integer","minimum":1,"maximum":100,"default":50},"cursor":{"type":"string","maxLength":85}}),&["id"]),true),
 ("engine_subscribe","Create an ephemeral subscription for up to 8 exact resources. Returns current samples and a 60-second lease. Poll to renew. Requires subscribe permission; samples may coalesce.",obj(json!({"ids":{"type":"array","items":id,"minItems":1,"maxItems":8,"uniqueItems":true}}),&["ids"]),true),
 ("engine_poll","Renew this MCP connection's subscription lease and return changed samples, including availability/evaluation changes. Does not trigger repeated getter evaluations.",subscription.clone(),true),
 ("engine_unsubscribe","Release this MCP connection's subscription and its producer demand. Other connections and server watches retain their own subscriptions.",subscription,true),
 ("engine_write","Write a stored Variable at expectedRevision. Returns durable audit/revision outcome. Reuse requestId only with identical arguments; operation_status reconciles uncertain responses without replay.",write.clone(),false),
 ("engine_set","Invoke an authored computed setter once at expectedRevision. Async I/O yields. Primary set and nested read/write permissions are rechecked at continuations; prior committed effects survive failure/cancellation.",write,false),
 ("engine_run","Admit an authorized server Pipeline. Returns durable audit plus admission/run ID, not completion. An exact requestId retry does not repeat effects. Explicitly admitted Pipelines retain server-owned lifetime.",action.clone(),false),
 ("engine_run_status","Read a durable run result using permission on the server-resolved owning Pipeline. Error messages are sanitized; intentionally returned values remain tagged.",obj(json!({"runId":id}),&["runId"]),true),
 ("engine_cancel_run","Cancel a run using permission on its server-resolved Pipeline. Already committed effects remain; an in-flight external effect can leave the run unknown. Audit completion means the cancellation request was handled.",obj(json!({"runId":id,"requestId":id}),&["runId","requestId"]),false),
 ("engine_resume_watch","Clear an authorized Watch fault so server checks can resume. Does not reset its state or repeat an old requestId.",action,false)
 ].into_iter().map(|(name,description,input,read)|json!({"name":name,"description":description,"inputSchema":input,"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":name!="engine_subscribe","openWorldHint":!read}})).collect()
}

#[cfg(test)]
mod tests;
