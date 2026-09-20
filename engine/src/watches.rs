use crate::{
    monitoring::{MonitorInstance, MonitorState},
    pipelines::{sample_wire, Pipelines},
    scheduling::request_key,
    script::Script,
    store::{Instance, Result},
    value,
};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    rc::Rc,
};
use tokio::time::{Duration, Instant};
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Clone)]
pub struct Watches {
    pub pipelines: Pipelines,
    active: Rc<RefCell<HashSet<String>>>,
    subscriptions: Rc<RefCell<HashSet<String>>>,
}
impl Watches {
    pub fn new(pipelines: Pipelines) -> Self {
        Self {
            pipelines,
            active: Default::default(),
            subscriptions: Default::default(),
        }
    }
    pub fn tick(&self) -> Result<()> {
        let e = &self.pipelines.engine;
        let config = e.store.borrow().monitoring_config()?;
        let mut wanted = HashSet::new();
        for i in &config.instances {
            if i.enabled && config.definition(&i.definition)?.kind == "watch" {
                wanted.extend(i.inputs.values().cloned());
            }
        }
        let existing = self.subscriptions.borrow().clone();
        for id in existing.difference(&wanted) {
            e.unsubscribe(id);
        }
        for id in wanted.difference(&existing) {
            e.subscribe(id)?;
            e.invalidate(id)?;
        }
        *self.subscriptions.borrow_mut() = wanted;
        for i in &config.instances {
            if !i.enabled
                || config.definition(&i.definition)?.kind != "watch"
                || self.active.borrow().contains(&i.id)
                || self.active.borrow().len() >= 64
            {
                continue;
            }
            let state = e.store.borrow().monitor_state(&i.id)?;
            if state.faulted {
                continue;
            }
            self.active.borrow_mut().insert(i.id.clone());
            let w = self.clone();
            let i = i.clone();
            tokio::task::spawn_local(async move {
                if let Err(error) = w.step(&i).await {
                    let s = w.pipelines.engine.store.borrow();
                    if let Ok(mut current) = s.monitor_state(&i.id) {
                        if current.generation == state.generation {
                            current.error = Some(error.chars().take(2048).collect());
                            current.faulted = true;
                            current.revision += 1;
                            let _ = s.save_monitor(&current);
                        }
                    }
                }
                w.active.borrow_mut().remove(&i.id);
            });
        }
        self.prune()
    }
    fn prune(&self) -> Result<()> {
        let s = self.pipelines.engine.store.borrow();
        let c = s.monitoring_config()?;
        let mut cursor = None;
        for i in &c.instances {
            if i.enabled && c.definition(&i.definition)?.kind == "watch" {
                let n = s.monitor_state(&i.id)?.cursor;
                cursor = Some(cursor.map_or(n, |v: u64| v.min(n)));
            }
        }
        s.conn
            .execute(
                "DELETE FROM monitoring_events WHERE seq<=?",
                [cursor.unwrap_or(u64::MAX.min(i64::MAX as u64))],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn resume(&self, id: &str) -> Result<()> {
        let s = self.pipelines.engine.store.borrow();
        let mut state = s.monitor_state(id)?;
        state.faulted = false;
        state.error = None;
        state.revision += 1;
        s.save_monitor(&state)
    }
    async fn step(&self, i: &MonitorInstance) -> Result<()> {
        let e = &self.pipelines.engine;
        let generation = e.store.borrow().monitor_state(&i.id)?.generation;
        for _ in 0..128 {
            let mut state = e.store.borrow().monitor_state(&i.id)?;
            if state.generation != generation {
                return Ok(());
            }
            let mut changed = vec![];
            let mut reason = "initial";
            let mut depth = 0;
            let event: Option<(u64, String, u32)> = e
                .store
                .borrow()
                .conn
                .query_row(
                    "SELECT seq,body,depth FROM monitoring_events WHERE seq>? ORDER BY seq LIMIT 1",
                    [state.cursor],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(err)?;
            let now = e.now();
            if state.evaluated {
                if let Some((seq, body, event_depth)) = event {
                    let v: Instance = serde_json::from_str(&body).map_err(err)?;
                    depth = event_depth;
                    for (alias, id) in &i.inputs {
                        if *id == v.id {
                            let old = state.inputs.get(alias);
                            if old.is_none_or(|o| {
                                o.value != v.value
                                    || o.quality != v.quality
                                    || o.has_value != v.has_value
                            }) {
                                changed.push(alias.clone());
                            }
                            state.inputs.insert(alias.clone(), v.clone());
                        }
                    }
                    state.cursor = seq;
                    reason = "input";
                    if changed.is_empty() {
                        e.store.borrow().save_monitor(&state)?;
                        continue;
                    }
                } else if let Some(schedule) = &i.schedule {
                    let due = state.next_due.unwrap_or(now);
                    if due > now {
                        return Ok(());
                    }
                    let (next, n) = schedule.advance(due, now)?;
                    state.next_due = Some(next);
                    state.last_due = Some(due);
                    state.missed = state.missed.saturating_add(n.saturating_sub(1));
                    reason = "timer";
                } else {
                    return Ok(());
                }
            } else {
                changed.extend(i.inputs.keys().cloned());
                if let Some(schedule) = &i.schedule {
                    state.next_due = Some(schedule.next_after(now)?);
                }
            }
            let config = e.store.borrow().monitoring_config()?;
            let d = config.definition(&i.definition)?;
            let result = tokio::time::timeout(
                Duration::from_millis(i.timeout_ms),
                self.evaluate(i, &state, &d.source, &changed, reason),
            )
            .await
            .map_err(|_| "Watch timeout")??;
            if !value::matches_schema(&result["state"], &d.state_schema) {
                return Err("Watch state schema".into());
            }
            let mut s = e.store.borrow_mut();
            let current = s.monitor_state(&i.id)?;
            if current.generation != state.generation {
                return Ok(());
            }
            if current.revision != state.revision {
                return Err("Watch state conflict".into());
            }
            state.state = result["state"].clone();
            state.revision += 1;
            state.evaluated = true;
            state.error = None;
            if !result["actions"]
                .as_array()
                .ok_or("Watch actions")?
                .is_empty()
            {
                state.last_actions.clear();
            }
            s.monitoring_atomic(|s| {
                let mut targets = HashSet::new();
                for action in result["actions"].as_array().ok_or("Watch action result")? {
                    let target = i
                        .actions
                        .get(action.as_str().ok_or("action alias")?)
                        .ok_or("action not granted")?;
                    if targets.insert(target) {
                        let key = request_key(
                            "watch",
                            &format!("{}:{target}", i.id),
                            state.generation,
                            state.revision as i64,
                        );
                        state
                            .last_actions
                            .push(s.admit(target, &key, "watch", depth + 1, now)?);
                    }
                }
                s.save_monitor(&state)
            })?;
            drop(s);
            self.pipelines.pump()?;
        }
        Ok(())
    }
    async fn evaluate(
        &self,
        i: &MonitorInstance,
        state: &MonitorState,
        source: &str,
        changed: &[String],
        reason: &str,
    ) -> Result<Value> {
        let e = &self.pipelines.engine;
        let samples: BTreeMap<_, _> = state
            .inputs
            .iter()
            .map(|(k, v)| (k, sample_wire(v)))
            .collect();
        let guest = Script::new()?;
        guest.eval(&format!("globalThis.definition=({source});globalThis.input={};",json!({"state":state.state,"params":i.params,"samples":samples,"changed":changed,"actions":i.actions.keys().collect::<Vec<_>>(),"reason":reason,"now":e.now()})))?;
        guest.eval(include_str!("../shared/watch.js"))?;
        let mut waits: futures_util::stream::FuturesUnordered<
            std::pin::Pin<Box<dyn std::future::Future<Output = Value>>>,
        > = Default::default();
        loop {
            if e.store.borrow().monitor_state(&i.id)?.generation != state.generation {
                return Err("Watch configuration replaced".into());
            }
            guest.drain()?;
            let result: Value =
                serde_json::from_str(&guest.string("JSON.stringify(invocation.result)")?)
                    .map_err(err)?;
            if !result.is_null() {
                if result["ok"] == true {
                    return Ok(result);
                }
                return Err(result["error"].as_str().unwrap_or("Watch failed").into());
            }
            let calls: Vec<Value> =
                serde_json::from_str(&guest.string("invocation.take()")?).map_err(err)?;
            for call in calls {
                let ms = call["ms"]
                    .as_u64()
                    .filter(|n| *n <= 300_000)
                    .ok_or("Watch sleep bounds")?;
                let id = call["id"].clone();
                let due = Instant::now() + Duration::from_millis(ms);
                waits.push(Box::pin(async move {
                    tokio::time::sleep_until(due).await;
                    id
                }));
            }
            use futures_util::StreamExt;
            if let Some(id) = waits.next().await {
                guest.eval(&format!(
                    "input.now={};invocation.receive({})",
                    e.now(),
                    json!({"id":id})
                ))?;
            } else {
                return Err("unresolved Watch promise".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{monitoring::MonitoringConfig, runtime::Engine, store::Store};
    use std::cell::Cell;
    fn fixture() -> (Watches, Rc<Cell<i64>>) {
        let mut s = Store::open(":memory:").unwrap();
        let mut v = crate::store::tests::seed(&mut s);
        v.value = value::number(12.0);
        v.timestamp = 1000;
        v.revision += 1;
        s.commit(1, &v, true, 1000).unwrap();
        let mut y = v.clone();
        y.id = "disk-y".into();
        y.revision = 1;
        y.value = value::number(20.0);
        s.add_instance(&y).unwrap();
        let config:MonitoringConfig=serde_json::from_value(json!({"version":1,"definitions":[{"id":"probe","version":1,"kind":"pipeline","source":"{async run(ctx){ctx.state.count=(ctx.state.count||0)+1;return {services:{one:50,two:30}}}}"},{"id":"disk","version":1,"kind":"watch","source":include_str!("../examples/disk-watch.js"),"parameter_change":"reset"}],"instances":[{"id":"probe-x","definition":"probe"},{"id":"probe-y","definition":"probe"},{"id":"watch-x","definition":"disk","inputs":{"disk":"metric"},"actions":{"investigate":"probe-x"}},{"id":"watch-y","definition":"disk","inputs":{"disk":"disk-y"},"actions":{"investigate":"probe-y"}}]})).unwrap();
        s.configure_monitoring(&config, 0).unwrap();
        let now = Rc::new(Cell::new(1000));
        let clock = now.clone();
        let e = Engine::with_clock(s, Rc::new(move || clock.get()));
        (Watches::new(Pipelines::new(e).unwrap()), now)
    }
    async fn settle(w: &Watches) {
        for _ in 0..8 {
            w.tick().unwrap();
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
    }
    fn set(w: &Watches, id: &str, n: f64) {
        let e = &w.pipelines.engine;
        let rev = e.store.borrow().instance(id).unwrap().revision;
        e.write(id, rev, value::number(n)).unwrap();
    }
    fn flag(w: &Watches, id: &str, key: &str) -> bool {
        let s = w.pipelines.engine.store.borrow().monitor_state(id).unwrap();
        let script = Script::new().unwrap();
        script
            .string(&format!(
                "String(TaliaValue.decode({}).{key}===true)",
                s.state
            ))
            .unwrap()
            == "true"
    }
    #[tokio::test]
    async fn ordered_crossings_independent_flags_rearming_and_reconfiguration() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (w, _) = fixture();
                settle(&w).await;
                set(&w, "metric", 5.0);
                set(&w, "metric", 15.0);
                settle(&w).await;
                assert_eq!(w.pipelines.engine.store.borrow().runs().unwrap().len(), 1);
                assert!(!flag(&w, "watch-x", "low"));
                assert!(!flag(&w, "watch-y", "low"));
                set(&w, "metric", 5.0);
                settle(&w).await;
                assert!(flag(&w, "watch-x", "low"));
                set(&w, "metric", 5.5);
                settle(&w).await;
                assert_eq!(w.pipelines.engine.store.borrow().runs().unwrap().len(), 2);
                let recovered = Watches::new(w.pipelines.clone());
                settle(&recovered).await;
                assert_eq!(w.pipelines.engine.store.borrow().runs().unwrap().len(), 2);
                let mut c = w
                    .pipelines
                    .engine
                    .store
                    .borrow()
                    .monitoring_config()
                    .unwrap();
                c.version = 2;
                c.instances[2].params = value::from_json(&json!({"low":7,"high":10,"recovery":12}));
                w.pipelines
                    .engine
                    .store
                    .borrow_mut()
                    .configure_monitoring(&c, 1)
                    .unwrap();
                settle(&w).await;
                assert_eq!(w.pipelines.engine.store.borrow().runs().unwrap().len(), 3);
            })
            .await;
    }
    #[tokio::test]
    async fn availability_timer_runs_without_new_observations() {
        tokio::task::LocalSet::new().run_until(async{
        let(w,now)=fixture();let mut c=w.pipelines.engine.store.borrow().monitoring_config().unwrap();
        c.version=2;c.definitions[1].version=2;c.definitions[1].source="{async evaluate(ctx){const v=await ctx.read('disk');if(ctx.now()-v.timestamp>500&&!ctx.state.missing){ctx.state.missing=true;await ctx.trigger('investigate')}}}".into();
        for i in c.instances.iter_mut().filter(|i|i.definition=="disk"){i.schedule=Some(crate::monitoring::Schedule::Interval{every_ms:100});}
        w.pipelines.engine.store.borrow_mut().configure_monitoring(&c,1).unwrap();settle(&w).await;assert!(w.pipelines.engine.store.borrow().runs().unwrap().is_empty());
        now.set(1600);settle(&w).await;assert_eq!(w.pipelines.engine.store.borrow().runs().unwrap().len(),2);
        now.set(2000);settle(&w).await;assert_eq!(w.pipelines.engine.store.borrow().runs().unwrap().len(),2);
    }).await;
    }
    #[tokio::test]
    async fn initial_critical_stale_freeze_and_atomic_action_failure() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (w, now) = fixture();
                set(&w, "metric", 5.0);
                // A new configuration captures the current critical baseline before its first evaluation.
                let mut c = w
                    .pipelines
                    .engine
                    .store
                    .borrow()
                    .monitoring_config()
                    .unwrap();
                c.version = 2;
                c.instances[2].params = value::from_json(&json!({"low":6}));
                w.pipelines
                    .engine
                    .store
                    .borrow_mut()
                    .configure_monitoring(&c, 1)
                    .unwrap();
                settle(&w).await;
                assert!(flag(&w, "watch-x", "low"));
                assert_eq!(w.pipelines.engine.store.borrow().runs().unwrap().len(), 1);
                now.set(100000);
                {
                    let mut s = w.pipelines.engine.store.borrow_mut();
                    let mut v = s.instance("metric").unwrap();
                    let rev = v.revision;
                    v.revision += 1;
                    v.value = value::number(20.0);
                    v.quality = "stale".into();
                    s.commit(rev, &v, false, now.get()).unwrap();
                }
                settle(&w).await;
                assert!(flag(&w, "watch-x", "low"));
                set(&w, "metric", 20.0);
                settle(&w).await;
                assert!(!flag(&w, "watch-x", "low"));
                c.version = 3;
                c.instances[0].enabled = false;
                w.pipelines
                    .engine
                    .store
                    .borrow_mut()
                    .configure_monitoring(&c, 2)
                    .unwrap();
                set(&w, "metric", 5.0);
                settle(&w).await;
                let state = w
                    .pipelines
                    .engine
                    .store
                    .borrow()
                    .monitor_state("watch-x")
                    .unwrap();
                assert!(state.faulted);
                assert!(!flag(&w, "watch-x", "low"));
                assert_eq!(w.pipelines.engine.store.borrow().runs().unwrap().len(), 1);
            })
            .await;
    }
}
