use crate::{
    script::Script,
    store::{Instance, Result, Store},
    value,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::watch,
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Default)]
pub struct Evaluation {
    pub running: usize,
    pub error: Option<String>,
    pub invalidated: bool,
    pub changed: HashSet<String>,
}
#[derive(Clone)]
pub struct Engine {
    pub store: Rc<RefCell<Store>>,
    pub evaluation: Rc<RefCell<HashMap<String, Evaluation>>>,
    shared: Rc<RefCell<HashMap<String, watch::Receiver<Option<Result<Instance>>>>>>,
    subscribers: Rc<RefCell<HashMap<String, usize>>>,
    clock: Rc<dyn Fn() -> i64>,
    runs: Rc<Cell<u64>>,
}
impl Engine {
    pub fn new(store: Store) -> Self {
        Self::with_clock(
            store,
            Rc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64
            }),
        )
    }
    pub fn with_clock(store: Store, clock: Rc<dyn Fn() -> i64>) -> Self {
        let recovered = store
            .instances()
            .unwrap_or_default()
            .into_iter()
            .filter(|i| {
                store
                    .definition(&i.definition)
                    .is_ok_and(|d| d.kind == "computed")
            })
            .map(|i| {
                (
                    i.id,
                    Evaluation {
                        invalidated: true,
                        ..Default::default()
                    },
                )
            })
            .collect();
        Self {
            store: Rc::new(RefCell::new(store)),
            evaluation: Rc::new(RefCell::new(recovered)),
            shared: Default::default(),
            subscribers: Default::default(),
            clock,
            runs: Default::default(),
        }
    }
    pub fn now(&self) -> i64 {
        (self.clock)()
    }
    pub fn runs(&self) -> u64 {
        self.runs.get()
    }
    pub fn subscribe(&self, id: &str) -> Result<()> {
        self.store.borrow().instance(id)?;
        *self.subscribers.borrow_mut().entry(id.into()).or_default() += 1;
        Ok(())
    }
    pub fn unsubscribe(&self, id: &str) {
        let mut s = self.subscribers.borrow_mut();
        if let Some(n) = s.get_mut(id) {
            *n -= 1;
            if *n == 0 {
                s.remove(id);
            }
        }
    }
    pub fn invalidate(&self, id: &str) -> Result<()> {
        let i = self.store.borrow().instance(id)?;
        if self.store.borrow().definition(&i.definition)?.kind == "stored" {
            return Ok(());
        }
        let mut next = i.clone();
        next.revision += 1;
        self.store
            .borrow_mut()
            .commit(i.revision, &next, false, self.now())?;
        let mut meta = self.evaluation.borrow_mut();
        meta.entry(id.into()).or_default().invalidated = true;
        drop(meta);
        self.refresh_subscribed(id);
        self.changed(id);
        Ok(())
    }
    fn refresh_subscribed(&self, id: &str) {
        if self.subscribers.borrow().contains_key(id) {
            let e = self.clone();
            let id = id.to_string();
            tokio::task::spawn_local(async move {
                let _ = e.read(&id, Duration::from_secs(5)).await;
            });
        }
    }
    pub fn changed(&self, id: &str) {
        let Ok(all) = self.store.borrow().instances() else {
            return;
        };
        let Ok(defs) = self.store.borrow().definitions() else {
            return;
        };
        let mut todo = vec![id.to_string()];
        let mut seen = HashSet::new();
        while let Some(changed) = todo.pop() {
            for i in &all {
                if defs
                    .iter()
                    .any(|d| d.id == i.definition && d.dependencies.contains(&changed))
                    && seen.insert(i.id.clone())
                {
                    let mut meta = self.evaluation.borrow_mut();
                    let m = meta.entry(i.id.clone()).or_default();
                    m.invalidated = true;
                    m.changed.insert(changed.clone());
                    drop(meta);
                    todo.push(i.id.clone());
                }
            }
        }
        for target in seen {
            self.refresh_subscribed(&target);
        }
    }
    pub fn write(&self, id: &str, expected: u64, v: Value) -> Result<Instance> {
        value::validate(&v)?;
        let mut i = self.store.borrow().instance(id)?;
        if self.store.borrow().definition(&i.definition)?.kind != "stored" {
            return Err("use computed setter".into());
        }
        i.revision += 1;
        i.value = v;
        i.has_value = true;
        i.timestamp = self.now();
        i.quality = "good".into();
        self.store
            .borrow_mut()
            .commit(expected, &i, true, self.now())?;
        self.changed(id);
        Ok(i)
    }
    pub fn read<'a>(
        &'a self,
        id: &'a str,
        timeout: Duration,
    ) -> Pin<Box<dyn Future<Output = Result<Instance>> + 'a>> {
        Box::pin(async move {
            if self
                .evaluation
                .borrow()
                .values()
                .map(|m| m.running)
                .sum::<usize>()
                >= 64
                && !self.shared.borrow().contains_key(id)
            {
                return Err("evaluation capacity".into());
            }
            let i = self.store.borrow().instance(id)?;
            let d = self.store.borrow().definition(&i.definition)?;
            if d.kind == "stored" {
                return Ok(i);
            }
            let mut rx = if d.read_policy == "shared" {
                let existing = self.shared.borrow().get(id).cloned();
                if let Some(rx) = existing {
                    rx
                } else {
                    self.start(id.to_string(), true)
                }
            } else {
                self.start(id.to_string(), false)
            };
            tokio::time::timeout(timeout, async {
                loop {
                    if let Some(r) = rx.borrow().clone() {
                        return r;
                    }
                    rx.changed().await.map_err(|_| "evaluation stopped")?;
                }
            })
            .await
            .map_err(|_| "read timeout".to_string())?
        })
    }
    fn start(&self, id: String, shared: bool) -> watch::Receiver<Option<Result<Instance>>> {
        let (tx, rx) = watch::channel(None);
        if shared {
            self.shared.borrow_mut().insert(id.clone(), rx.clone());
        }
        let e = self.clone();
        self.evaluation
            .borrow_mut()
            .entry(id.clone())
            .or_default()
            .running += 1;
        tokio::task::spawn_local(async move {
            let end = Instant::now() + Duration::from_secs(5);
            let result = loop {
                let r = tokio::time::timeout_at(end, e.evaluate(&id, "get", value::undefined()))
                    .await
                    .unwrap_or_else(|_| Err("evaluation timeout".into()));
                if r.as_ref().err().is_some_and(|s| s == "conflict") && Instant::now() < end {
                    tokio::task::yield_now().await;
                    continue;
                }
                break r;
            };
            {
                let mut all = e.evaluation.borrow_mut();
                let m = all.entry(id.clone()).or_default();
                m.running -= 1;
                m.error = result.as_ref().err().cloned();
                if result.is_ok() {
                    m.invalidated = false;
                    m.changed.clear();
                }
            }
            if shared {
                e.shared.borrow_mut().remove(&id);
            }
            let _ = tx.send(Some(result));
        });
        rx
    }
    pub async fn set(&self, id: &str, arg: Value) -> Result<Instance> {
        value::validate(&arg)?;
        tokio::time::timeout(Duration::from_secs(5), self.evaluate(id, "set", arg))
            .await
            .map_err(|_| "setter timeout".to_string())?
    }
    fn fence(&self, start: &Instance, deps: &HashMap<String, u64>) -> Result<()> {
        let i = self
            .store
            .borrow()
            .instance(&start.id)
            .map_err(|_| "cancelled")?;
        if i.generation != start.generation {
            return Err("cancelled".into());
        }
        if i.revision != start.revision {
            return Err("conflict".into());
        }
        for (id, revision) in deps {
            if self.store.borrow().instance(id)?.revision != *revision {
                return Err("conflict".into());
            }
        }
        Ok(())
    }
    async fn evaluate(&self, id: &str, mode: &str, argument: Value) -> Result<Instance> {
        self.runs.set(self.runs.get() + 1);
        let mut start = self.store.borrow().instance(id)?;
        let definition = self.store.borrow().definition(&start.definition)?;
        let mut deps = HashMap::new();
        for dep in &definition.dependencies {
            deps.insert(dep.clone(), self.store.borrow().instance(dep)?.revision);
        }
        let changed: Vec<_> = self
            .evaluation
            .borrow()
            .get(id)
            .map(|m| m.changed.iter().cloned().collect())
            .unwrap_or_default();
        let guest = Script::new()?;
        guest.eval(&format!("globalThis.definition=({});globalThis.input={};",definition.source,json!({"state":start.state,"params":start.params,"argument":argument,"mode":mode,"changed":changed,"now":self.now()})))?;
        guest.eval(include_str!("../shared/computed.js"))?;
        loop {
            self.fence(&start, &deps)?;
            guest.drain()?;
            let result: Value =
                serde_json::from_str(&guest.string("JSON.stringify(invocation.result)")?)
                    .map_err(|e| e.to_string())?;
            if !result.is_null() {
                if result["ok"] != true {
                    return Err(result["error"]
                        .as_str()
                        .unwrap_or("evaluation failed")
                        .into());
                }
                let mut next = start.clone();
                next.state = result["state"].clone();
                next.value = result["value"].clone();
                next.has_value = true;
                next.timestamp = self.now();
                next.quality = "good".into();
                next.revision += 1;
                self.fence(&start, &deps)?;
                self.store
                    .borrow_mut()
                    .commit(start.revision, &next, true, self.now())?;
                self.changed(id);
                return Ok(next);
            }
            let calls: Vec<Value> = serde_json::from_str(&guest.string("invocation.take()")?)
                .map_err(|e| e.to_string())?;
            if calls.is_empty() {
                return Err("unresolved promise without host work".into());
            }
            if calls.len() > 64 {
                return Err("host call limit".into());
            }
            let dispatched = Instant::now();
            let mut reads = HashMap::new();
            for call in &calls {
                if call["op"] == "read" {
                    let target = call["arg"]["value"][1]
                        .as_str()
                        .ok_or("read id")?
                        .to_string();
                    if !definition.dependencies.contains(&target) {
                        return Err("undeclared read dependency".into());
                    }
                    let e = self.clone();
                    let key = call["id"].to_string();
                    reads.insert(
                        key,
                        tokio::task::spawn_local(async move {
                            e.read(&target, Duration::from_secs(5)).await
                        }),
                    );
                }
            }
            for call in calls {
                self.fence(&start, &deps)?;
                let arg = &call["arg"];
                value::validate(arg)?;
                // Decode only the host operation arguments; all user values remain tagged.
                let plain: String = guest.string(&format!(
                    "JSON.stringify(TaliaValue.decode({arg})) ?? 'null'"
                ))?;
                let plain: Value = serde_json::from_str(&plain).map_err(|e| e.to_string())?;
                let answer: Result<Value> = match call["op"].as_str().unwrap_or("") {
                    "sleep" => {
                        let ms = plain
                            .as_u64()
                            .filter(|n| *n <= 5000)
                            .ok_or("sleep bounds")?;
                        tokio::time::sleep_until(dispatched + Duration::from_millis(ms)).await;
                        Ok(value::undefined())
                    }
                    "read" => {
                        let target = plain.as_str().ok_or("read id")?;
                        if !definition.dependencies.iter().any(|d| d == target) {
                            return Err("undeclared read dependency".into());
                        }
                        let v = reads
                            .remove(&call["id"].to_string())
                            .ok_or("read dispatch")?
                            .await
                            .map_err(|_| "read task failed")??;
                        deps.insert(target.into(), v.revision);
                        Ok(v.value)
                    }
                    "write" => {
                        let target = plain["id"].as_str().ok_or("write id")?;
                        if mode != "set" || !definition.dependencies.iter().any(|d| d == target) {
                            return Err("write capability denied".into());
                        }
                        let wire: Value = serde_json::from_str(&guest.string(&format!(
                            "TaliaValue.stringify(TaliaValue.decode({arg}).value)"
                        ))?)
                        .map_err(|e| e.to_string())?;
                        let expected = self.store.borrow().instance(target)?.revision;
                        let v = self.write(target, expected, wire)?;
                        deps.insert(target.into(), v.revision);
                        Ok(v.value)
                    }
                    "commit" => {
                        let mut next = start.clone();
                        next.state = arg.clone();
                        next.revision += 1;
                        self.store
                            .borrow_mut()
                            .commit(start.revision, &next, false, self.now())?;
                        start = next;
                        Ok(value::undefined())
                    }
                    _ => Err("unknown host operation".into()),
                };
                self.fence(&start, &deps)?;
                let response = match answer {
                    Ok(v) => json!({"id":call["id"],"value":v}),
                    Err(e) => json!({"id":call["id"],"error":e}),
                };
                guest.eval(&format!(
                    "input.now={};invocation.receive({});",
                    self.now(),
                    serde_json::to_string(&response.to_string()).unwrap()
                ))?;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{tests::seed, Definition};
    pub(super) fn fixture(source: &str) -> Engine {
        let mut s = Store::open(":memory:").unwrap();
        let mut i = seed(&mut s);
        let d = Definition {
            id: "calc".into(),
            version: 1,
            source: source.into(),
            kind: "computed".into(),
            value_schema: "any".into(),
            state_schema: "any".into(),
            dependencies: vec!["metric".into()],
            read_policy: "shared".into(),
        };
        s.define(&d, 0, None).unwrap();
        i.id = "calc".into();
        i.definition = "calc".into();
        i.state = value::number(0.0);
        i.has_value = false;
        s.add_instance(&i).unwrap();
        Engine::with_clock(s, Rc::new(|| 1234))
    }
    #[tokio::test(flavor = "current_thread")]
    async fn shared_reads_survive_waiter_cancellation() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let e = fixture(
                    "{async get(c){await c.sleep(30);c.state++;return await c.read('metric')}}",
                );
                let a = e.clone();
                let b = e.clone();
                let t = tokio::task::spawn_local(async move {
                    a.read("calc", Duration::from_secs(1)).await
                });
                tokio::task::yield_now().await;
                t.abort();
                let r = b.read("calc", Duration::from_secs(1)).await.unwrap();
                assert_eq!(e.runs(), 1);
                assert_eq!(r.timestamp, 1234);
                assert_eq!(r.state["value"][1].as_f64(), Some(1.0));
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn setter_interleaves_and_stale_getter_retries() {
        tokio::task::LocalSet::new().run_until(async{let e=fixture("{async get(c){await c.sleep(30);return await c.read('metric')},set(c,v){c.state=v;return v}}");let a=e.clone();let t=tokio::task::spawn_local(async move{a.read("calc",Duration::from_secs(1)).await});tokio::time::sleep(Duration::from_millis(5)).await;let r=e.set("calc",value::number(4.0)).await.unwrap();assert_eq!(r.state["value"][1].as_f64(),Some(4.0));let r=t.await.unwrap().unwrap();assert_eq!(r.state["value"][1].as_f64(),Some(4.0));assert_eq!(e.runs(),3);}).await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn failed_setter_preserves_only_committed_state_and_effects() {
        tokio::task::LocalSet::new().run_until(async{let e=fixture("{get(c){return undefined},async set(c,v){c.state=5;await c.commit();await c.write('metric',Infinity);c.state=9;throw Error('fail')}}");assert!(e.set("calc",value::undefined()).await.is_err());let s=e.store.borrow();assert_eq!(s.instance("calc").unwrap().state["value"][1].as_f64(),Some(5.0));assert_eq!(s.instance("metric").unwrap().value,value::number(f64::INFINITY));}).await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn definition_update_cancels_old_work() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let e = fixture("{async get(c){await c.sleep(30);return 9}}");
                let a = e.clone();
                let t = tokio::task::spawn_local(async move {
                    a.read("calc", Duration::from_secs(1)).await
                });
                tokio::time::sleep(Duration::from_millis(5)).await;
                let mut d = e.store.borrow().definition("calc").unwrap();
                d.version = 2;
                d.source = "{get(){return NaN}}".into();
                e.store.borrow_mut().define(&d, 1, None).unwrap();
                assert_eq!(t.await.unwrap().unwrap_err(), "cancelled");
                assert!(!e.store.borrow().instance("calc").unwrap().has_value);
                let r = e.read("calc", Duration::from_secs(1)).await.unwrap();
                assert_eq!(r.value, value::number(f64::NAN));
            })
            .await;
    }
}
#[cfg(test)]
mod more_tests {
    use super::tests::fixture;
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn dependency_changes_retry_and_expose_changed_inputs() {
        tokio::task::LocalSet::new().run_until(async{let e=fixture("{async get(c){const v=await c.read('metric');await c.sleep(30);c.state=c.changed;return v}}");let a=e.clone();let t=tokio::task::spawn_local(async move{a.read("calc",Duration::from_secs(1)).await});tokio::time::sleep(Duration::from_millis(10)).await;e.write("metric",1,value::number(8.0)).unwrap();let r=t.await.unwrap().unwrap();assert_eq!(r.value["value"][1].as_f64(),Some(8.0));assert_eq!(e.runs(),2);assert!(r.state.to_string().contains("metric"));}).await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn last_reader_timeout_does_not_cancel_producer() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let e = fixture("{async get(c){await c.sleep(30);return null}}");
                assert!(e.read("calc", Duration::from_millis(5)).await.is_err());
                tokio::time::sleep(Duration::from_millis(50)).await;
                assert!(e.store.borrow().instance("calc").unwrap().has_value);
                assert_eq!(e.runs(), 1);
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn stale_setter_is_not_replayed() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let e = fixture(
                    "{get(){return 0},async set(c,v){await c.sleep(30);c.state=v;return v}}",
                );
                let a = e.clone();
                let t = tokio::task::spawn_local(
                    async move { a.set("calc", value::number(9.0)).await },
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
                e.store
                    .borrow_mut()
                    .reconfigure("calc", 1, value::undefined())
                    .unwrap();
                assert_eq!(t.await.unwrap().unwrap_err(), "cancelled");
                assert_eq!(e.runs(), 1);
                assert_eq!(
                    e.store.borrow().instance("calc").unwrap().state,
                    value::number(0.0)
                );
            })
            .await;
    }
}
#[cfg(test)]
mod invalidation_tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn explicit_invalidation_fences_old_cache_result() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let e = super::tests::fixture(
                    "{async get(c){await c.sleep(30);c.state++;return c.state}}",
                );
                let a = e.clone();
                let t = tokio::task::spawn_local(async move {
                    a.read("calc", Duration::from_secs(1)).await
                });
                tokio::time::sleep(Duration::from_millis(5)).await;
                e.invalidate("calc").unwrap();
                let r = t.await.unwrap().unwrap();
                assert_eq!(e.runs(), 2);
                assert_eq!(r.state["value"][1].as_f64(), Some(1.0));
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn independent_policy_starts_separate_evaluations() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let e = super::tests::fixture("{async get(c){await c.sleep(10);return c.state}}");
                let mut d = e.store.borrow().definition("calc").unwrap();
                d.version = 2;
                d.read_policy = "independent".into();
                e.store.borrow_mut().define(&d, 1, None).unwrap();
                let (a, b) = tokio::join!(
                    e.read("calc", Duration::from_secs(1)),
                    e.read("calc", Duration::from_secs(1))
                );
                assert!(a.is_ok() && b.is_ok());
                assert!(e.runs() >= 2);
            })
            .await;
    }
}
