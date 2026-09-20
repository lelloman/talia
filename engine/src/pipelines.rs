//! Durable admission and asynchronous Pipeline execution on the engine's local executor.
use crate::{
    definitions::identifier,
    monitoring::{MonitorInstance, MonitorState},
    runtime::Engine,
    script::Script,
    sources::{Adapters, SourceRequest},
    store::{Instance, Result, Store},
    value,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    rc::Rc,
};
use tokio::{sync::Notify, time::Duration};
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub instance: String,
    pub generation: u64,
    pub status: String,
    pub cause: String,
    pub created: i64,
    pub started: Option<i64>,
    pub finished: Option<i64>,
    pub attempt: u32,
    pub error: Option<String>,
    pub result: Value,
    pub effect_pending: bool,
    #[serde(default)]
    pub pending_effects: u32,
    pub depth: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Admission {
    pub disposition: String,
    pub run_id: Option<String>,
}
impl Store {
    pub(crate) fn monitoring_atomic<T>(
        &mut self,
        f: impl FnOnce(&mut Store) -> Result<T>,
    ) -> Result<T> {
        self.conn
            .execute_batch("SAVEPOINT monitoring_batch")
            .map_err(err)?;
        match f(self) {
            Ok(v) => {
                self.conn
                    .execute_batch("RELEASE monitoring_batch")
                    .map_err(err)?;
                Ok(v)
            }
            Err(e) => {
                self.conn
                    .execute_batch("ROLLBACK TO monitoring_batch; RELEASE monitoring_batch")
                    .map_err(err)?;
                Err(e)
            }
        }
    }
    pub fn run(&self, id: &str) -> Result<Run> {
        let body: String = self
            .conn
            .query_row("SELECT body FROM monitoring_runs WHERE id=?", [id], |r| {
                r.get(0)
            })
            .map_err(err)?;
        serde_json::from_str(&body).map_err(err)
    }
    pub fn runs(&self) -> Result<Vec<Run>> {
        let mut q = self
            .conn
            .prepare("SELECT body FROM monitoring_runs ORDER BY rowid DESC LIMIT 1000")
            .map_err(err)?;
        let result = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .map(|s| serde_json::from_str(&s.map_err(err)?).map_err(err))
            .collect();
        result
    }
    pub(crate) fn save_run(&self, r: &Run) -> Result<()> {
        self.conn.execute("INSERT INTO monitoring_runs VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET status=excluded.status,body=excluded.body",params![r.id,r.instance,r.status,serde_json::to_string(r).map_err(err)?]).map_err(err)?;
        self.bump_monitoring()
    }
    pub(crate) fn bump_monitoring(&self) -> Result<()> {
        self.conn
            .execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])
            .map_err(err)?;
        Ok(())
    }
    pub(crate) fn save_monitor(&self, s: &MonitorState) -> Result<()> {
        self.conn
            .execute(
                "UPDATE monitoring_state SET body=? WHERE id=?",
                params![serde_json::to_string(s).map_err(err)?, s.id],
            )
            .map_err(err)?;
        self.bump_monitoring()
    }
    pub fn admit(
        &mut self,
        target: &str,
        request_id: &str,
        cause: &str,
        depth: u32,
        now: i64,
    ) -> Result<Admission> {
        identifier(request_id)?;
        identifier(target)?;
        if !["manual", "scheduled", "watch", "recovery"].contains(&cause) || depth > 16 {
            return Err("run cause/chain limit".into());
        }
        let signature = json!([target, cause, depth]).to_string();
        let prior: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT signature,body FROM monitoring_requests WHERE id=?",
                [request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        if let Some((sig, body)) = prior {
            if sig != signature {
                return Err("run request argument conflict".into());
            }
            return serde_json::from_str(&body).map_err(err);
        }
        let config = self.monitoring_config()?;
        let i = config.instance(target)?;
        if !i.enabled || config.definition(&i.definition)?.kind != "pipeline" {
            return Err("Pipeline disabled/unavailable".into());
        }
        let generation = self.monitor_state(target)?.generation;
        self.monitoring_atomic(|s|{
   let active:Option<String>=s.conn.query_row("SELECT id FROM monitoring_runs WHERE instance=? AND status IN ('pending','running') ORDER BY rowid LIMIT 1",[target],|r|r.get(0)).optional().map_err(err)?;
   let (disposition,existing,status)=match (active,cause){
    (Some(id),"manual")=>("already_running",Some(id),""),
    (Some(_),"scheduled"|"recovery")=>("skipped",None,""),
    (Some(_),"watch")=>{let queued:Option<String>=s.conn.query_row("SELECT id FROM monitoring_runs WHERE instance=? AND status='queued' LIMIT 1",[target],|r|r.get(0)).optional().map_err(err)?;("queued",queued,"queued")},
    _=>("accepted",None,"pending"),
   };
   let id=if existing.is_some() || status.is_empty(){existing}else{
    if s.conn.query_row("SELECT count(*) FROM monitoring_runs WHERE status IN ('pending','running','queued')",[],|r|r.get::<_,u32>(0)).map_err(err)?>=128{return Err("run capacity".into());}
    s.conn.execute("UPDATE metadata SET value=value+1 WHERE key='run_sequence'",[]).map_err(err)?;
    let seq:u64=s.conn.query_row("SELECT value FROM metadata WHERE key='run_sequence'",[],|r|r.get(0)).map_err(err)?;
    let id=format!("run-{seq}");s.save_run(&Run{id:id.clone(),instance:target.into(),generation,status:status.into(),cause:cause.into(),created:now,started:None,finished:None,attempt:0,error:None,result:value::undefined(),effect_pending:false,pending_effects:0,depth})?;Some(id)
   };
   let a=Admission{disposition:disposition.into(),run_id:id};s.conn.execute("INSERT INTO monitoring_requests VALUES(?,?,?)",params![request_id,signature,serde_json::to_string(&a).map_err(err)?]).map_err(err)?;Ok(a)
  })
    }
    pub fn recover_runs(&mut self, now: i64) -> Result<()> {
        self.monitoring_atomic(|s| {
            // Read active records directly; terminal history limits must never hide unfinished work.
            let mut q = s
                .conn
                .prepare("SELECT id FROM monitoring_runs WHERE status='running'")
                .map_err(err)?;
            let ids = q
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(err)?;
            drop(q);
            for id in ids {
                let mut r = s.run(&id)?;
                r.status = "unknown".into();
                r.error = Some("server restarted during execution".into());
                r.finished = Some(now);
                s.save_run(&r)?;
            }
            Ok(())
        })
    }
    pub fn pending_runs(&mut self) -> Result<Vec<Run>> {
        let mut q=self.conn.prepare("SELECT id FROM monitoring_runs WHERE status IN ('pending','queued') ORDER BY rowid").map_err(err)?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)?;
        drop(q);
        let mut out = vec![];
        for id in ids {
            let mut r = self.run(&id)?;
            let busy:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM monitoring_runs WHERE instance=? AND status IN ('running','pending') AND id!=?)",params![r.instance,r.id],|r|r.get(0)).map_err(err)?;
            if !busy {
                if r.status == "queued" {
                    r.status = "pending".into();
                    self.save_run(&r)?;
                }
                out.push(r);
            }
        }
        Ok(out)
    }
}
#[derive(Clone)]
pub struct Pipelines {
    pub engine: Engine,
    pub adapters: Adapters,
    active: Rc<RefCell<HashMap<String, Rc<Notify>>>>,
}
impl Pipelines {
    pub fn new(engine: Engine) -> Result<Self> {
        Ok(Self {
            engine,
            adapters: Adapters::new()?,
            active: Default::default(),
        })
    }
    pub fn request(
        &self,
        target: &str,
        request_id: &str,
        cause: &str,
        depth: u32,
    ) -> Result<Admission> {
        let a = self.engine.store.borrow_mut().admit(
            target,
            request_id,
            cause,
            depth,
            self.engine.now(),
        )?;
        self.pump()?;
        Ok(a)
    }
    pub fn pump(&self) -> Result<()> {
        let ready = self.engine.store.borrow_mut().pending_runs()?;
        for mut r in ready {
            if self.active.borrow().len() >= 64 {
                break;
            }
            let valid = {
                let s = self.engine.store.borrow();
                s.monitor_state(&r.instance)
                    .is_ok_and(|m| m.generation == r.generation)
                    && s.monitoring_config()?
                        .instance(&r.instance)
                        .is_ok_and(|i| i.enabled)
            };
            if !valid {
                r.status = "cancelled".into();
                r.error = Some("configuration replaced".into());
                r.finished = Some(self.engine.now());
                self.engine.store.borrow().save_run(&r)?;
                continue;
            }
            r.status = "running".into();
            r.started = Some(self.engine.now());
            self.engine.store.borrow().save_run(&r)?;
            let cancel = Rc::new(Notify::new());
            self.active
                .borrow_mut()
                .insert(r.id.clone(), cancel.clone());
            let p = self.clone();
            tokio::task::spawn_local(async move {
                let timeout = p
                    .engine
                    .store
                    .borrow()
                    .monitoring_config()
                    .and_then(|c| Ok(c.instance(&r.instance)?.timeout_ms))
                    .unwrap_or(1);
                let result = tokio::select! { x=tokio::time::timeout(Duration::from_millis(timeout),p.execute(&r))=>x.unwrap_or_else(|_|Err("run timeout".into())), _=cancel.notified()=>Err("run cancelled".into()) };
                if let Err(e) = result {
                    let _ = p.fail(&r.id, &e);
                }
                p.active.borrow_mut().remove(&r.id);
                let _ = p.pump();
            });
        }
        Ok(())
    }
    pub fn cancel(&self, id: &str) -> Result<Run> {
        let r = self.engine.store.borrow().run(id)?;
        if ["pending", "queued", "running"].contains(&r.status.as_str()) {
            self.fail(id, "run cancelled")?;
            if let Some(n) = self.active.borrow().get(id) {
                n.notify_one();
            }
        }
        self.engine.store.borrow().run(id)
    }
    fn fail(&self, id: &str, error: &str) -> Result<()> {
        let mut s = self.engine.store.borrow_mut();
        let mut r = s.run(id)?;
        if !["pending", "queued", "running"].contains(&r.status.as_str()) {
            return Ok(());
        }
        let was_running = r.status == "running";
        r.status = if r.effect_pending {
            "unknown"
        } else if error == "run cancelled" || error == "configuration replaced" {
            "cancelled"
        } else if error == "run timeout" {
            "timed_out"
        } else {
            "failed"
        }
        .into();
        r.error = Some(error.chars().take(2048).collect());
        r.finished = Some(self.engine.now());
        let result = s.monitoring_atomic(|s| {
            s.save_run(&r)?;
            if let Ok(mut m) = s.monitor_state(&r.instance) {
                if was_running && m.generation == r.generation {
                    m.error = r.error.clone();
                    m.revision += 1;
                    s.save_monitor(&m)?;
                    if let Ok(c) = s.monitoring_config() {
                        if let Ok(i) = c.instance(&r.instance) {
                            for output in i.outputs.values() {
                                let mut v = s.instance(output)?;
                                if v.quality != "error" {
                                    let rev = v.revision;
                                    v.revision += 1;
                                    v.quality = "error".into();
                                    s.commit(rev, &v, false, self.engine.now())?;
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        });
        if let Err(storage_error) = result {
            // Backpressure on observation publication must not leave a run falsely running.
            r.error = Some(format!("{error}; quality publication: {storage_error}"));
            s.save_run(&r)?;
            if let Ok(mut m) = s.monitor_state(&r.instance) {
                if was_running && m.generation == r.generation {
                    m.error = r.error.clone();
                    m.revision += 1;
                    s.save_monitor(&m)?;
                }
            }
        }
        Ok(())
    }
    pub(crate) fn fence(
        &self,
        r: &Run,
        state: &MonitorState,
        deps: &BTreeMap<String, u64>,
    ) -> Result<()> {
        let s = self.engine.store.borrow();
        let active = s.run(&r.id)?;
        if active.status != "running" {
            return Err("run cancelled".into());
        }
        let current = s
            .monitor_state(&r.instance)
            .map_err(|_| "configuration replaced")?;
        if current.generation != r.generation {
            return Err("configuration replaced".into());
        }
        if current.revision != state.revision {
            return Err("state conflict".into());
        }
        for (id, rev) in deps {
            if s.instance(id)?.revision != *rev {
                return Err("dependency conflict".into());
            }
        }
        Ok(())
    }
    async fn execute(&self, r: &Run) -> Result<()> {
        let c = self.engine.store.borrow().monitoring_config()?;
        let i = c.instance(&r.instance)?.clone();
        for attempt in 0..=i.retries {
            {
                let mut run = self.engine.store.borrow().run(&r.id)?;
                run.attempt = attempt + 1;
                self.engine.store.borrow().save_run(&run)?;
            }
            match self.evaluate(r, &i).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let run = self.engine.store.borrow().run(&r.id)?;
                    if attempt == i.retries
                        || !i.repeat_safe
                        || run.effect_pending
                        || [
                            "configuration replaced",
                            "run cancelled",
                            "state conflict",
                            "dependency conflict",
                            "output conflict",
                        ]
                        .contains(&e.as_str())
                    {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_millis(i.retry_delay_ms)).await;
                }
            }
        }
        unreachable!()
    }
    fn publish(
        &self,
        r: &Run,
        state: &mut MonitorState,
        new_state: Value,
        outputs: &mut BTreeMap<String, Value>,
        expected: &mut BTreeMap<String, u64>,
        deps: &BTreeMap<String, u64>,
        result: Option<Value>,
    ) -> Result<()> {
        self.fence(r, state, deps)?;
        let now = self.engine.now();
        let mut s = self.engine.store.borrow_mut();
        let c = s.monitoring_config()?;
        let i = c.instance(&r.instance)?;
        let d = c.definition(&i.definition)?;
        if !value::matches_schema(&new_state, &d.state_schema) {
            return Err("Pipeline state schema".into());
        }
        let mut next = s.monitor_state(&state.id)?;
        next.state = new_state;
        next.revision += 1;
        next.error = None;
        s.monitoring_atomic(|s| {
            for (alias, wire) in outputs.iter() {
                let id = i.outputs.get(alias).ok_or("output not granted")?;
                let mut v = s.instance(id)?;
                if expected.get(id) != Some(&v.revision) {
                    return Err("output conflict".into());
                }
                let rev = v.revision;
                v.revision += 1;
                v.value = wire.clone();
                v.has_value = true;
                v.timestamp = now;
                v.quality = "good".into();
                s.commit(rev, &v, true, now)?;
                s.conn.execute("UPDATE monitoring_events SET depth=? WHERE seq=(SELECT max(seq) FROM monitoring_events) AND body=?",params![r.depth,serde_json::to_string(&v).map_err(err)?]).map_err(err)?;
            }
            s.save_monitor(&next)?;
            if let Some(result) = result {
                let mut run = s.run(&r.id)?;
                if run.effect_pending {
                    return Err("unknown external outcome".into());
                }
                run.result = result;
                run.status = "complete".into();
                run.finished = Some(now);
                s.save_run(&run)?;
            }
            Ok(())
        })?;
        for alias in outputs.keys() {
            let id = &i.outputs[alias];
            expected.insert(id.clone(), s.instance(id)?.revision);
        }
        drop(s);
        for alias in outputs.keys() {
            self.engine.changed(&i.outputs[alias]);
        }
        outputs.clear();
        *state = next;
        Ok(())
    }
    async fn evaluate(&self, r: &Run, i: &MonitorInstance) -> Result<()> {
        let c = self.engine.store.borrow().monitoring_config()?;
        let d = c.definition(&i.definition)?;
        let mut state = self.engine.store.borrow().monitor_state(&r.instance)?;
        let mut expected = BTreeMap::new();
        for id in i.outputs.values() {
            expected.insert(
                id.clone(),
                self.engine.store.borrow().instance(id)?.revision,
            );
        }
        let mut deps = BTreeMap::new();
        let mut outputs = BTreeMap::new();
        let guest = Script::new()?;
        guest.eval(&format!(
            "globalThis.definition=({});globalThis.input={};",
            d.source,
            json!({"state":state.state,"params":i.params,"now":self.engine.now()})
        ))?;
        guest.eval(include_str!("../shared/pipeline.js"))?;
        // Start independent reads, source calls and waits together. Dropping this set cancels I/O.
        type HostFuture = std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(Value, Option<(String, u64)>)>>>,
        >;
        let mut futures: futures_util::stream::FuturesUnordered<
            std::pin::Pin<
                Box<
                    dyn std::future::Future<
                        Output = (Value, Result<(Value, Option<(String, u64)>)>),
                    >,
                >,
            >,
        > = Default::default();
        loop {
            self.fence(r, &state, &deps)?;
            guest.drain()?;
            let result: Value =
                serde_json::from_str(&guest.string("JSON.stringify(invocation.result)")?)
                    .map_err(err)?;
            if !result.is_null() {
                if result["ok"] != true {
                    return Err(result["error"].as_str().unwrap_or("Pipeline failed").into());
                }
                return self.publish(
                    r,
                    &mut state,
                    result["state"].clone(),
                    &mut outputs,
                    &mut expected,
                    &deps,
                    Some(result["value"].clone()),
                );
            }
            let calls: Vec<Value> =
                serde_json::from_str(&guest.string("invocation.take()")?).map_err(err)?;
            if calls.is_empty() && futures.is_empty() {
                return Err("unresolved Pipeline promise".into());
            }
            if calls.len() > 64 {
                return Err("host call capacity".into());
            }
            let mut immediate = false;
            for call in calls {
                self.fence(r, &state, &deps)?;
                let alias = call["alias"].as_str().unwrap_or("");
                let future: Option<HostFuture> = match call["op"].as_str().unwrap_or("") {
                    "source" => {
                        let source = c
                            .source(i.sources.get(alias).ok_or("source not granted")?)?
                            .clone();
                        let req: SourceRequest = serde_json::from_value(call["request"].clone())
                            .map_err(|_| "source request")?;
                        let effect = req.has_effect();
                        if effect {
                            let mut run = self.engine.store.borrow().run(&r.id)?;
                            run.pending_effects += 1;
                            run.effect_pending = true;
                            self.engine.store.borrow().save_run(&run)?;
                        }
                        let adapters = self.adapters.clone();
                        let engine = self.engine.clone();
                        let id = r.id.clone();
                        Some(Box::pin(async move {
                            let answer = adapters.fetch(&source, &req).await;
                            if effect && answer.is_ok() {
                                let mut run = engine.store.borrow().run(&id)?;
                                if run.status == "running" {
                                    run.pending_effects = run.pending_effects.saturating_sub(1);
                                    run.effect_pending = run.pending_effects > 0;
                                    engine.store.borrow().save_run(&run)?;
                                }
                            }
                            answer.map(|v| (v, None))
                        }))
                    }
                    "read" => {
                        let id = i.inputs.get(alias).ok_or("input not granted")?.clone();
                        let e = self.engine.clone();
                        Some(Box::pin(async move {
                            let v = e.read(&id, Duration::from_millis(300_000)).await?;
                            Ok((sample_wire(&v), Some((id, v.revision))))
                        }))
                    }
                    "sleep" => {
                        let ms = call["ms"]
                            .as_u64()
                            .filter(|x| *x <= 300_000)
                            .ok_or("sleep bounds")?;
                        Some(Box::pin(async move {
                            tokio::time::sleep(Duration::from_millis(ms)).await;
                            Ok((value::undefined(), None))
                        }))
                    }
                    "publish" => {
                        if !i.outputs.contains_key(alias) {
                            return Err("output not granted".into());
                        }
                        value::validate(&call["value"])?;
                        outputs.insert(alias.into(), call["value"].clone());
                        None
                    }
                    "commit" => {
                        self.publish(
                            r,
                            &mut state,
                            call["state"].clone(),
                            &mut outputs,
                            &mut expected,
                            &deps,
                            None,
                        )?;
                        None
                    }
                    _ => return Err("unknown Pipeline operation".into()),
                };
                let call_id = call["id"].clone();
                if let Some(f) = future {
                    futures.push(Box::pin(async move { (call_id, f.await) }));
                } else {
                    immediate = true;
                    guest.eval(&format!(
                        "invocation.receive({})",
                        json!({"id":call_id,"value":value::undefined()})
                    ))?;
                }
            }
            if immediate {
                continue;
            }
            use futures_util::StreamExt;
            if let Some((id, answer)) = futures.next().await {
                self.fence(r, &state, &deps)?;
                let response = match answer {
                    Ok((value, dep)) => {
                        if let Some((key, rev)) = dep {
                            deps.insert(key, rev);
                        }
                        json!({"id":id,"value":value})
                    }
                    Err(error) => json!({"id":id,"error":error}),
                };
                guest.eval(&format!(
                    "input.now={};invocation.receive({response})",
                    self.engine.now()
                ))?;
            }
        }
    }
}
pub fn sample_wire(v: &Instance) -> Value {
    let mut wire = value::from_json(
        &json!({"id":v.id,"value":null,"hasValue":v.has_value,"timestamp":v.timestamp,"quality":v.quality,"revision":v.revision}),
    );
    for pair in wire["value"][1].as_array_mut().unwrap() {
        if pair[0] == "value" {
            pair[1] = v.value["value"].clone();
        }
    }
    wire
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn fixture(source: &str) -> Pipelines {
        let mut s = Store::open(":memory:").unwrap();
        crate::store::tests::seed(&mut s);
        let mut c = crate::monitoring::tests::config();
        c.definitions[0].source = source.into();
        s.configure_monitoring(&c, 0).unwrap();
        Pipelines::new(Engine::new(s)).unwrap()
    }
    pub async fn finish(p: &Pipelines, id: &str) -> Run {
        for _ in 0..200 {
            let r = p.engine.store.borrow().run(id).unwrap();
            if !["pending", "running", "queued"].contains(&r.status.as_str()) {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("run did not finish")
    }
    #[tokio::test]
    async fn admission_and_async_io() {
        tokio::task::LocalSet::new().run_until(async{
  let p=fixture("{async run(ctx){await Promise.all([ctx.sleep(40),ctx.sleep(40)]);ctx.state.n=(ctx.state.n||0)+1;await ctx.publish('metric',ctx.state.n);return ctx.state.n}}");
  let first=p.request("x","a","manual",0).unwrap().run_id.unwrap();
  assert_eq!(p.request("x","b","manual",0).unwrap().run_id,Some(first.clone()));
  assert_eq!(p.request("x","c","scheduled",0).unwrap().disposition,"skipped");
  let queued=p.request("x","d","watch",1).unwrap().run_id.unwrap();assert_ne!(first,queued);
  assert_eq!(p.request("x","e","watch",1).unwrap().run_id,Some(queued.clone()));
  assert!(p.request("y","a","manual",0).is_err());
  assert_eq!(finish(&p,&first).await.status,"complete");assert_eq!(finish(&p,&queued).await.status,"complete");
  assert_eq!(p.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),Some(2.0));
 }).await;
    }
    #[tokio::test]
    async fn cancellation_generation_and_prior_commits() {
        tokio::task::LocalSet::new().run_until(async{
  let p=fixture("{async run(ctx){await ctx.publish('metric',7);await ctx.commit();await ctx.sleep(100);await ctx.publish('metric',99)}}");
  let id=p.request("x","a","manual",0).unwrap().run_id.unwrap();tokio::time::sleep(Duration::from_millis(20)).await;
  p.cancel(&id).unwrap();assert_eq!(finish(&p,&id).await.status,"cancelled");
  tokio::time::sleep(Duration::from_millis(110)).await;assert_eq!(p.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),Some(7.0));
  let id=p.request("x","b","manual",0).unwrap().run_id.unwrap();tokio::time::sleep(Duration::from_millis(20)).await;
  let mut c=p.engine.store.borrow().monitoring_config().unwrap();c.version+=1;c.instances[0].params=value::number(2.0);p.engine.store.borrow_mut().configure_monitoring(&c,1).unwrap();
  assert_eq!(finish(&p,&id).await.status,"cancelled");
 }).await;
    }
    #[tokio::test]
    async fn retries_deadline_and_atomic_failure() {
        tokio::task::LocalSet::new().run_until(async{
  let p=fixture("{async run(ctx){ctx.state.n=(ctx.state.n||0)+1;await ctx.commit();if(ctx.state.n<2)throw Error('retry');await ctx.publish('metric',42)}}");
  let mut c=p.engine.store.borrow().monitoring_config().unwrap();c.version=2;c.instances[0].retries=1;c.instances[0].repeat_safe=true;p.engine.store.borrow_mut().configure_monitoring(&c,1).unwrap();
  let id=p.request("x","retry","manual",0).unwrap().run_id.unwrap();let r=finish(&p,&id).await;assert_eq!(r.status,"complete");assert_eq!(r.attempt,2);
  c.version=3;c.definitions[0].version=2;c.definitions[0].source="{async run(ctx){await ctx.publish('metric',99);await ctx.sleep(100)}}".into();c.instances[0].timeout_ms=15;p.engine.store.borrow_mut().configure_monitoring(&c,2).unwrap();
  let id=p.request("x","timeout","manual",0).unwrap().run_id.unwrap();assert_eq!(finish(&p,&id).await.status,"timed_out");assert_eq!(p.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),Some(42.0));
 }).await;
    }
    #[tokio::test]
    async fn fast_continuation_does_not_wait_for_unrelated_io() {
        tokio::task::LocalSet::new().run_until(async{
        let p=fixture("{async run(ctx){const slow=ctx.sleep(100);await ctx.sleep(5);await ctx.publish('metric',7);await ctx.commit();await slow}}");
        let id=p.request("x","continuation","manual",0).unwrap().run_id.unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(p.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),Some(7.0));
        assert_eq!(p.engine.store.borrow().run(&id).unwrap().status,"running");
        assert_eq!(finish(&p,&id).await.status,"complete");
    }).await;
    }
    #[tokio::test]
    async fn cancelling_queued_work_does_not_invalidate_active_run() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let p =
                    fixture("{async run(ctx){await ctx.sleep(60);await ctx.publish('metric',7)}}");
                let active = p
                    .request("x", "active", "manual", 0)
                    .unwrap()
                    .run_id
                    .unwrap();
                let queued = p
                    .request("x", "queued", "watch", 1)
                    .unwrap()
                    .run_id
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
                p.cancel(&queued).unwrap();
                assert_eq!(finish(&p, &active).await.status, "complete");
                assert_eq!(
                    p.engine.store.borrow().run(&queued).unwrap().status,
                    "cancelled"
                );
                assert_eq!(
                    p.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),
                    Some(7.0)
                );
            })
            .await;
    }
    #[tokio::test]
    async fn removed_and_recreated_instance_fences_old_work() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let p =
                    fixture("{async run(ctx){await ctx.sleep(60);await ctx.publish('metric',99)}}");
                let id = p.request("x", "old", "manual", 0).unwrap().run_id.unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
                let mut c = p.engine.store.borrow().monitoring_config().unwrap();
                let x = c.instances.remove(0);
                c.version = 2;
                p.engine
                    .store
                    .borrow_mut()
                    .configure_monitoring(&c, 1)
                    .unwrap();
                c.version = 3;
                c.instances.push(x);
                p.engine
                    .store
                    .borrow_mut()
                    .configure_monitoring(&c, 2)
                    .unwrap();
                assert_eq!(finish(&p, &id).await.status, "cancelled");
                assert_eq!(
                    p.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),
                    Some(1.0)
                );
            })
            .await;
    }
    #[test]
    fn recovery_distinguishes_pending_from_unknown() {
        let p = fixture("{async run(ctx){}}");
        let mut s = p.engine.store.borrow_mut();
        let a = s.admit("x", "a", "manual", 0, 1).unwrap().run_id.unwrap();
        let b = s.admit("y", "b", "manual", 0, 1).unwrap().run_id.unwrap();
        let mut r = s.run(&a).unwrap();
        r.status = "running".into();
        r.effect_pending = true;
        s.save_run(&r).unwrap();
        s.recover_runs(2).unwrap();
        assert_eq!(s.run(&a).unwrap().status, "unknown");
        assert_eq!(s.run(&b).unwrap().status, "pending");
        assert_eq!(s.admit("x", "a", "manual", 0, 3).unwrap().run_id, Some(a));
    }
}

#[cfg(test)]
mod effect_tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_effect_remains_unknown_and_never_replays() {
        tokio::task::LocalSet::new().run_until(async{
  use axum::{Router,routing::post};use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
  let count=Arc::new(AtomicUsize::new(0));let hits=count.clone();
  let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let url=format!("http://{}",listener.local_addr().unwrap());
  let server=tokio::spawn(async move{axum::serve(listener,Router::new().route("/effect",post(move ||{let hits=hits.clone();async move{hits.fetch_add(1,Ordering::SeqCst);tokio::time::sleep(Duration::from_millis(100)).await;"{}"}}))).await.unwrap()});
  let p=tests::fixture("{async run(ctx){await ctx.source('probe',{kind:'http',path:'/effect',method:'POST'});await ctx.publish('metric',99)}}");
  let mut c=p.engine.store.borrow().monitoring_config().unwrap();c.version=2;c.sources.push(crate::monitoring::DataSource{id:"probe".into(),kind:"http".into(),url,credential_ref:None,timeout_ms:1000,max_bytes:1024});c.instances[0].sources.insert("probe".into(),"probe".into());p.engine.store.borrow_mut().configure_monitoring(&c,1).unwrap();
  let id=p.request("x","effect","manual",0).unwrap().run_id.unwrap();
  for _ in 0..100{if count.load(Ordering::SeqCst)>0{break;}tokio::time::sleep(Duration::from_millis(2)).await;}
  assert_eq!(count.load(Ordering::SeqCst),1);assert_eq!(p.cancel(&id).unwrap().status,"unknown");
  tokio::time::sleep(Duration::from_millis(120)).await;assert_eq!(count.load(Ordering::SeqCst),1);
  assert_eq!(p.request("x","effect","manual",0).unwrap().run_id,Some(id));assert_eq!(count.load(Ordering::SeqCst),1);
  assert_eq!(p.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),Some(1.0));server.abort();
 }).await;
    }
}

impl Store {
    pub fn run_admission(&self, id: &str) -> Result<Option<Admission>> {
        let body: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM monitoring_requests WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        body.map(|b| serde_json::from_str(&b).map_err(err))
            .transpose()
    }
    /// Read-only synthetic resources, using the same lossless sample contract as Variables.
    pub fn monitoring_values(&self) -> Result<Vec<Value>> {
        let config = self.monitoring_config()?;

        let mut out = vec![];
        for i in &config.instances {
            let state = self.monitor_state(&i.id)?;
            let latest_id: Option<String> = self
                .conn
                .query_row(
                    "SELECT id FROM monitoring_runs WHERE instance=? ORDER BY rowid DESC LIMIT 1",
                    [&i.id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(err)?;
            let latest_run = latest_id.map(|id| self.run(&id)).transpose()?;
            let latest = latest_run.as_ref();
            let mut payload = value::from_json(
                &json!({"state":null,"error":state.error,"faulted":state.faulted,"nextDue":state.next_due,"missed":state.missed,"lastGap":state.last_gap,"lastActions":state.last_actions,"run":latest}),
            );
            for pair in payload["value"][1].as_array_mut().unwrap() {
                if pair[0] == "state" {
                    pair[1] = state.state["value"].clone();
                }
            }
            // Run.result is itself tagged; clients can decode that explicitly when inspecting run history.
            out.push(json!({"id":format!("monitor.{}",i.id),"value":payload,"hasValue":true,"timestamp":latest.and_then(|r|r.finished.or(r.started)).unwrap_or(0),"quality":if state.error.is_some(){"error"}else{"good"},"revision":self.revision()?,"generation":state.generation,"evaluation":if state.faulted{"error"}else{"idle"},"error":state.error}));
        }
        Ok(out)
    }
}
