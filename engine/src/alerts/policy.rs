use super::*;
use crate::{runtime::Engine, script::Script, value};
use std::{cell::RefCell, collections::HashSet, rc::Rc, time::Duration};
fn yes() -> bool {
    true
}
fn interval() -> u64 {
    1000
}
fn attempts() -> u32 {
    3
}
fn retry() -> u64 {
    30000
}
fn ttl() -> u64 {
    3600000
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseAction {
    pub id: String,
    pub destinations: Vec<String>,
    #[serde(default)]
    pub delay_ms: u64,
    #[serde(default)]
    pub repeat_ms: Option<u64>,
    #[serde(default = "yes")]
    pub until_ack: bool,
    #[serde(default = "attempts")]
    pub max_attempts: u32,
    #[serde(default = "retry")]
    pub retry_ms: u64,
    #[serde(default = "ttl")]
    pub expiry_ms: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    #[serde(default)]
    pub reset_ack: bool,
    #[serde(default)]
    pub actions: Vec<ResponseAction>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub id: String,
    pub version: u64,
    pub source: String,
    pub stages: BTreeMap<String, Stage>,
    #[serde(default)]
    pub recovery: Vec<ResponseAction>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: String,
    pub version: u64,
    pub policy: String,
    pub key: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default = "interval")]
    pub every_ms: u64,
    #[serde(default = "yes")]
    pub enabled: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Evaluation {
    pub state: Value,
    #[serde(default)]
    pub actions: Option<Vec<String>>,
    pub error: Option<String>,
    pub last_at: Option<i64>,
    pub next_at: i64,
    pub binding_version: u64,
    pub policy_version: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Silence {
    pub id: String,
    pub version: u64,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub until: i64,
    pub reason: String,
    #[serde(default)]
    pub actor: String,
}
fn validate_actions(actions: &[ResponseAction]) -> Result<()> {
    let mut ids = HashSet::new();
    if actions.len() > 32 {
        return Err("action capacity".into());
    }
    for a in actions {
        key(&a.id)?;
        if !ids.insert(&a.id)
            || a.destinations.len() > 32
            || a.destinations.is_empty()
            || a.max_attempts == 0
            || a.max_attempts > 100
            || a.retry_ms < 100
            || a.retry_ms > 86400000
            || a.delay_ms > 31536000000
            || a.repeat_ms
                .is_some_and(|n| !(1000..=31536000000).contains(&n))
            || a.expiry_ms == 0
            || a.expiry_ms > 2419200000
        {
            return Err("invalid response action".into());
        }
        for d in &a.destinations {
            key(d)?;
        }
    }
    Ok(())
}
impl Store {
    pub fn alert_policies(&self) -> Result<Vec<Policy>> {
        self.alert_list("policy")
    }
    pub fn alert_bindings(&self) -> Result<Vec<Binding>> {
        self.alert_list("binding")
    }
    pub fn alert_policy_save(
        &mut self,
        p: &Policy,
        expected: u64,
        migrations: &BTreeMap<String, String>,
        actor: &str,
        now: i64,
    ) -> Result<()> {
        key(&p.id)?;
        if p.version != expected + 1 || p.stages.is_empty() || p.stages.len() > 32 {
            return Err("invalid policy version or stages".into());
        }
        let script = Script::new()?;
        script.eval(&format!("globalThis.definition=({}); if(typeof definition.evaluate!=='function') throw Error('evaluate required');",p.source))?;
        for (name, stage) in &p.stages {
            key(name)?;
            validate_actions(&stage.actions)?;
        }
        validate_actions(&p.recovery)?;
        for (from, to) in migrations {
            key(from)?;
            if !p.stages.contains_key(to) {
                return Err("invalid migration stage".into());
            }
        }
        self.alert_atomic(|s| {
            if s.alert_get::<Policy>("policy", &p.id)?
                .map_or(0, |p| p.version)
                != expected
            {
                return Err("conflict".into());
            }
            for b in s.alert_bindings()?.into_iter().filter(|b| b.policy == p.id) {
                if let Some(mut a) = s.alert_get::<Alert>("alert", &b.key)? {
                    if a.active && !p.stages.contains_key(&a.stage) {
                        a.stage = migrations
                            .get(&a.stage)
                            .ok_or("occupied stage requires migration")?
                            .clone();
                        a.stage_at = now;
                        a.updated_at = now;
                        a.revision += 1;
                        if p.stages[&a.stage].reset_ack {
                            a.acknowledgement = None;
                        }
                        s.alert_put("alert", &a.key, &a)?;
                        s.alert_put("occurrence", &format!("{}:{}", a.key, a.occurrence), &a)?;
                    }
                }
                let mut eval = s
                    .alert_get::<Evaluation>("evaluation", &b.id)?
                    .unwrap_or_default();
                eval.next_at = now;
                eval.last_at = None;
                s.alert_put("evaluation", &b.id, &eval)?;
            }
            s.alert_put("policy", &p.id, p)?;
            s.alert_audit(
                actor,
                "policy_saved",
                &p.id,
                now,
                json!({"version":p.version}),
            )
        })
    }
    pub fn alert_binding_save(
        &mut self,
        b: &Binding,
        expected: u64,
        actor: &str,
        now: i64,
    ) -> Result<()> {
        key(&b.id)?;
        key(&b.key)?;
        if b.version != expected + 1
            || !(100..=86400000).contains(&b.every_ms)
            || b.inputs.len() > 32
            || b.labels.len() > 32
            || b.params.to_string().len() > 32768
        {
            return Err("invalid binding".into());
        }
        for (alias, id) in &b.inputs {
            key(alias)?;
            self.instance(id)?;
        }
        if self.alert_get::<Policy>("policy", &b.policy)?.is_none() {
            return Err("policy not found".into());
        }
        self.alert_atomic(|s| {
            let old = s.alert_get::<Binding>("binding", &b.id)?;
            if old.as_ref().map_or(0, |v| v.version) != expected {
                return Err("conflict".into());
            }
            if old
                .as_ref()
                .is_some_and(|v| v.key != b.key || v.policy != b.policy)
            {
                return Err("binding identity and policy are immutable".into());
            }
            if s.alert_bindings()?
                .iter()
                .any(|other| other.id != b.id && other.key == b.key)
            {
                return Err("alert key already bound".into());
            }
            let mut eval = s
                .alert_get::<Evaluation>("evaluation", &b.id)?
                .unwrap_or_default();
            eval.next_at = now;
            eval.last_at = None;
            s.alert_put("binding", &b.id, b)?;
            s.alert_put("evaluation", &b.id, &eval)?;
            s.alert_audit(
                actor,
                "binding_saved",
                &b.id,
                now,
                json!({"version":b.version}),
            )
        })
    }
    pub fn alert_silences(&self) -> Result<Vec<Silence>> {
        self.alert_list("silence")
    }
    pub fn alert_silence_save(
        &mut self,
        v: &Silence,
        expected: u64,
        actor: &str,
        now: i64,
    ) -> Result<()> {
        key(&v.id)?;
        if v.version != expected + 1
            || v.until < now
            || v.until.saturating_sub(now) > 31536000000
            || v.reason.is_empty()
            || v.reason.len() > 1024
            || v.labels.len() > 32
            || (v.key.is_none() && v.labels.is_empty())
        {
            return Err("invalid silence".into());
        }
        self.alert_atomic(|s| {
            if s.alert_get::<Silence>("silence", &v.id)?
                .map_or(0, |v| v.version)
                != expected
            {
                return Err("conflict".into());
            }
            let mut v = v.clone();
            v.actor = actor.into();
            s.alert_put("silence", &v.id, &v)?;
            s.alert_audit(actor, "silence_saved", &v.id, now, json!({"until":v.until}))
        })
    }
    pub fn alert_silenced(&self, a: &Alert, now: i64) -> Result<bool> {
        Ok(self.alert_silences()?.iter().any(|s| {
            s.until > now
                && s.key.as_ref().is_none_or(|k| *k == a.key)
                && s.labels.iter().all(|(k, v)| a.labels.get(k) == Some(v))
        }))
    }
    pub fn alert_evaluations(&self) -> Result<Vec<Value>> {
        self.alert_bindings()?.iter().map(|b|Ok(json!({"id":b.id,"evaluation":self.alert_get::<Evaluation>("evaluation",&b.id)?}))).collect()
    }
    pub fn alert_recover_policies(&mut self, now: i64) -> Result<()> {
        for b in self.alert_bindings()? {
            let mut e = self
                .alert_get::<Evaluation>("evaluation", &b.id)?
                .unwrap_or_default();
            e.last_at = None;
            e.next_at = now;
            self.alert_put("evaluation", &b.id, &e)?;
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Policies {
    pub engine: Engine,
    active: Rc<RefCell<HashSet<String>>>,
}
impl Policies {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            active: Default::default(),
        }
    }
    pub fn tick(&self) -> Result<()> {
        let now = self.engine.now();
        for b in self.engine.store.borrow().alert_bindings()? {
            if !b.enabled
                || self.active.borrow().len() >= 32
                || self.active.borrow().contains(&b.id)
            {
                continue;
            }
            let eval = self
                .engine
                .store
                .borrow()
                .alert_get::<Evaluation>("evaluation", &b.id)?
                .unwrap_or_default();
            if eval.next_at > now {
                continue;
            }
            self.active.borrow_mut().insert(b.id.clone());
            let this = self.clone();
            tokio::task::spawn_local(async move {
                let result = this.evaluate(&b).await;
                if let Err(error) = result {
                    let s = this.engine.store.borrow();
                    if s.alert_get::<Binding>("binding", &b.id)
                        .ok()
                        .flatten()
                        .is_some_and(|v| v.version == b.version)
                    {
                        let mut e = s
                            .alert_get::<Evaluation>("evaluation", &b.id)
                            .ok()
                            .flatten()
                            .unwrap_or_default();
                        e.error = Some(error.chars().take(1024).collect());
                        e.next_at = this.engine.now() + b.every_ms as i64;
                        let _ = s.alert_put("evaluation", &b.id, &e);
                    }
                }
                this.active.borrow_mut().remove(&b.id);
            });
        }
        Ok(())
    }
    pub async fn evaluate(&self, b: &Binding) -> Result<()> {
        let p = self
            .engine
            .store
            .borrow()
            .alert_get::<Policy>("policy", &b.policy)?
            .ok_or("policy missing")?;
        let mut evaluation = self
            .engine
            .store
            .borrow()
            .alert_get::<Evaluation>("evaluation", &b.id)?
            .unwrap_or_default();
        let old = self
            .engine
            .store
            .borrow()
            .alert_get::<Alert>("alert", &b.key)?;
        let mut samples = BTreeMap::new();
        for (alias, id) in &b.inputs {
            let i = self.engine.read(id, Duration::from_secs(5)).await?;
            samples.insert(alias.clone(),json!({"value":i.value,"quality":i.quality,"timestamp":i.timestamp,"hasValue":i.has_value,"revision":i.revision}));
        }
        let now = self.engine.now();
        let guest = Script::new()?;
        guest.eval(&format!("globalThis.definition=({});globalThis.input={};",p.source,json!({"params":b.params,"state":evaluation.state,"samples":samples,"alert":old,"now":now})))?;
        guest.eval(r#"globalThis.answer=null;globalThis.problem=null;
 const ctx={params:input.params,state:input.state==null?{}:TaliaValue.decode(input.state),alert:input.alert,now:()=>input.now,
 read:async name=>{if(!Object.prototype.hasOwnProperty.call(input.samples,name))throw Error('input not granted');let s=input.samples[name];return {...s,value:TaliaValue.decode(s.value)};}};
 Promise.resolve().then(()=>definition.evaluate(ctx)).then(result=>{answer={result,state:TaliaValue.encode(ctx.state)}},()=>{problem='policy evaluation failed'});"#)?;
        guest.drain()?;
        let output: Value =
            serde_json::from_str(&guest.string("JSON.stringify({answer,problem})")?)
                .map_err(err)?;
        if !output["problem"].is_null() || output["answer"].is_null() {
            return Err("policy evaluation failed or unresolved promise".into());
        }
        let r = &output["answer"]["result"];
        let active = r["active"].as_bool().ok_or("active must be boolean")?;
        let stage = r["stage"].as_str().ok_or("stage required")?;
        let cfg = p.stages.get(stage).ok_or("stage not declared")?;
        let reset =
            active && cfg.reset_ack && old.as_ref().is_none_or(|a| a.stage != stage || !a.active);
        let o = Observation {
            key: b.key.clone(),
            active,
            stage: stage.into(),
            severity: r["severity"].as_str().unwrap_or("warning").into(),
            message: r["message"].as_str().unwrap_or("").into(),
            labels: b.labels.clone(),
            reset_ack: reset,
        };
        evaluation.actions = if r.get("actions").is_some() {
            let actions: Vec<String> = serde_json::from_value(r["actions"].clone()).map_err(err)?;
            let allowed = if active { &cfg.actions } else { &p.recovery };
            if actions.len() > 32
                || actions
                    .iter()
                    .any(|id| !allowed.iter().any(|a| a.id == *id))
            {
                return Err("action not declared".into());
            }
            Some(actions)
        } else {
            None
        };
        evaluation.state = output["answer"]["state"].clone();
        value::validate(&evaluation.state)?;
        evaluation.error = None;
        evaluation.last_at = Some(now);
        evaluation.next_at = now + b.every_ms as i64;
        evaluation.binding_version = b.version;
        evaluation.policy_version = p.version;
        let mut s = self.engine.store.borrow_mut();
        s.alert_atomic(|s| {
            if s.alert_get::<Binding>("binding", &b.id)?
                .is_none_or(|v| v.version != b.version)
                || s.alert_get::<Policy>("policy", &b.policy)?
                    .is_none_or(|v| v.version != p.version)
            {
                return Err("configuration changed".into());
            }
            for (alias, id) in &b.inputs {
                if s.instance(id)?.revision
                    != samples[alias]["revision"]
                        .as_u64()
                        .ok_or("sample revision")?
                {
                    return Err("input changed".into());
                }
            }
            s.alert_observe(
                &o,
                old.as_ref().map_or(0, |a| a.revision),
                &format!("policy:{}", b.id),
                now,
            )?;
            s.alert_put("evaluation", &b.id, &evaluation)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Policies, Binding) {
        let mut s = Store::open(":memory:").unwrap();
        crate::store::tests::seed(&mut s);
        let p=Policy{id:"disk".into(),version:1,source:"{async evaluate(ctx){let v=await ctx.read('disk');ctx.state.count=(ctx.state.count||0)+1;return {active:v.value<10,stage:v.value<6?'critical':'warning',message:'disk low'}}}".into(),stages:[("warning".into(),Stage::default()),("critical".into(),Stage{reset_ack:true,actions:vec![]})].into_iter().collect(),recovery:vec![]};
        s.alert_policy_save(&p, 0, &BTreeMap::new(), "admin", 0)
            .unwrap();
        let b = Binding {
            id: "disk-a".into(),
            version: 1,
            policy: "disk".into(),
            key: "disk:a".into(),
            params: json!({}),
            inputs: [("disk".into(), "metric".into())].into_iter().collect(),
            labels: BTreeMap::new(),
            every_ms: 1000,
            enabled: true,
        };
        s.alert_binding_save(&b, 0, "admin", 0).unwrap();
        (Policies::new(Engine::new(s)), b)
    }
    #[tokio::test]
    async fn policies_references_migration_and_silence() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (p, b) = fixture();
                p.evaluate(&b).await.unwrap();
                let a = p.engine.store.borrow().alert(&b.key).unwrap();
                assert_eq!(a.stage, "critical");
                let mut b2 = b.clone();
                b2.id = "disk-b".into();
                b2.key = "disk:b".into();
                p.engine
                    .store
                    .borrow_mut()
                    .alert_binding_save(&b2, 0, "admin", 0)
                    .unwrap();
                p.evaluate(&b2).await.unwrap();
                let mut policy = p.engine.store.borrow().alert_policies().unwrap().remove(0);
                policy.version = 2;
                policy.stages.remove("critical");
                assert!(p
                    .engine
                    .store
                    .borrow_mut()
                    .alert_policy_save(&policy, 1, &BTreeMap::new(), "admin", 0)
                    .is_err());
                p.engine
                    .store
                    .borrow_mut()
                    .alert_policy_save(
                        &policy,
                        1,
                        &[("critical".into(), "warning".into())]
                            .into_iter()
                            .collect(),
                        "admin",
                        1,
                    )
                    .unwrap();
                assert!(p
                    .engine
                    .store
                    .borrow()
                    .alerts()
                    .unwrap()
                    .iter()
                    .all(|a| a.stage == "warning"));
                let a = p.engine.store.borrow().alert(&b.key).unwrap();
                let silence = Silence {
                    id: "maintenance".into(),
                    version: 1,
                    key: Some(a.key.clone()),
                    labels: BTreeMap::new(),
                    until: 10,
                    reason: "test".into(),
                    actor: String::new(),
                };
                p.engine
                    .store
                    .borrow_mut()
                    .alert_silence_save(&silence, 0, "human", 2)
                    .unwrap();
                assert!(p.engine.store.borrow().alert_silenced(&a, 3).unwrap());
                assert!(!p.engine.store.borrow().alert_silenced(&a, 10).unwrap());
                p.engine
                    .store
                    .borrow_mut()
                    .alert_recover_policies(11)
                    .unwrap();
                assert!(p
                    .engine
                    .store
                    .borrow()
                    .alert_get::<Evaluation>("evaluation", &b.id)
                    .unwrap()
                    .unwrap()
                    .last_at
                    .is_none());
            })
            .await;
    }
    #[tokio::test]
    async fn invalid_result_and_infinite_loop_never_change_alert() {
        let (p, b) = fixture();
        let mut policy = p.engine.store.borrow().alert_policies().unwrap().remove(0);
        policy.version = 2;
        policy.source = "{evaluate(){while(true){}}}".into();
        p.engine
            .store
            .borrow_mut()
            .alert_policy_save(&policy, 1, &BTreeMap::new(), "admin", 0)
            .unwrap();
        assert!(p.evaluate(&b).await.is_err());
        assert!(p.engine.store.borrow().alerts().unwrap().is_empty());
    }
}
