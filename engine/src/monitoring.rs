//! Server-owned monitoring configuration. Activation never awaits and is all-or-nothing.
use crate::{
    definitions::identifier,
    script::Script,
    store::{Result, Store},
    value,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

fn timeout() -> u64 {
    30_000
}
fn response_limit() -> usize {
    1_048_576
}
fn yes() -> bool {
    true
}
fn any() -> String {
    "any".into()
}
fn preserve() -> String {
    "preserve".into()
}
pub fn empty_state() -> Value {
    json!({"version":1,"value":["object",[]]})
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DataSource {
    pub id: String,
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub credential_ref: Option<String>,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
    #[serde(default = "response_limit")]
    pub max_bytes: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Schedule {
    Interval {
        every_ms: u64,
    },
    Daily {
        time: String,
        zone: String,
        #[serde(default)]
        weekdays: Vec<u32>,
    },
}
impl Schedule {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Interval { every_ms } if (100..=31_536_000_000).contains(every_ms) => Ok(()),
            Self::Daily {
                time,
                zone,
                weekdays,
            } => {
                chrono::NaiveTime::parse_from_str(time, "%H:%M").map_err(err)?;
                zone.parse::<chrono_tz::Tz>().map_err(err)?;
                if weekdays.len() > 7 || weekdays.iter().any(|d| !(1..=7).contains(d)) {
                    return Err("weekdays must be ISO 1..7".into());
                }
                Ok(())
            }
            _ => Err("schedule interval bounds".into()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MonitorDefinition {
    pub id: String,
    pub version: u64,
    pub kind: String,
    pub source: String,
    #[serde(default = "any")]
    pub state_schema: String,
    #[serde(default = "empty_state")]
    pub initial_state: Value,
    #[serde(default = "preserve")]
    pub parameter_change: String,
    #[serde(default)]
    pub migrate: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MonitorInstance {
    pub id: String,
    pub definition: String,
    #[serde(default = "empty_state")]
    pub params: Value,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    #[serde(default)]
    pub actions: BTreeMap<String, String>,
    #[serde(default)]
    pub schedule: Option<Schedule>,
    #[serde(default)]
    pub stale_after_ms: Option<u64>,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub retries: u32,
    #[serde(default)]
    pub retry_delay_ms: u64,
    #[serde(default)]
    pub repeat_safe: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MonitoringConfig {
    pub version: u64,
    #[serde(default)]
    pub sources: Vec<DataSource>,
    #[serde(default)]
    pub definitions: Vec<MonitorDefinition>,
    #[serde(default)]
    pub instances: Vec<MonitorInstance>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonitorState {
    pub id: String,
    pub generation: u64,
    pub revision: u64,
    pub state: Value,
    pub error: Option<String>,
    pub next_due: Option<i64>,
    pub last_due: Option<i64>,
    pub missed: u64,
    #[serde(default)]
    pub observed: BTreeMap<String, u64>,
}
impl MonitoringConfig {
    pub fn instance(&self, id: &str) -> Result<&MonitorInstance> {
        self.instances
            .iter()
            .find(|i| i.id == id)
            .ok_or_else(|| "monitoring instance missing".into())
    }
    pub fn definition(&self, id: &str) -> Result<&MonitorDefinition> {
        self.definitions
            .iter()
            .find(|i| i.id == id)
            .ok_or_else(|| "monitoring definition missing".into())
    }
    pub fn source(&self, id: &str) -> Result<&DataSource> {
        self.sources
            .iter()
            .find(|i| i.id == id)
            .ok_or_else(|| "data source missing".into())
    }
    pub fn validate(&self, store: &Store) -> Result<()> {
        if self.sources.len() > 256
            || self.definitions.len() > 256
            || self.instances.len() > 1024
            || serde_json::to_vec(self).map_err(err)?.len() > 2_097_152
        {
            return Err("monitoring configuration capacity".into());
        }
        for ids in [
            self.sources.iter().map(|x| &x.id).collect::<Vec<_>>(),
            self.definitions.iter().map(|x| &x.id).collect(),
            self.instances.iter().map(|x| &x.id).collect(),
        ] {
            let mut seen = HashSet::new();
            for id in ids {
                identifier(id)?;
                if !seen.insert(id) {
                    return Err("duplicate monitoring identifier".into());
                }
            }
        }
        for s in &self.sources {
            let u = url::Url::parse(&s.url).map_err(|_| "invalid source URL")?;
            if !["http", "https"].contains(&u.scheme())
                || !u.username().is_empty()
                || u.password().is_some()
                || u.query().is_some()
                || u.fragment().is_some()
            {
                return Err("source URL must be HTTP(S) without credentials/query/fragment".into());
            }
            if !["http", "prometheus"].contains(&s.kind.as_str())
                || !(1..=300_000).contains(&s.timeout_ms)
                || !(1..=8_388_608).contains(&s.max_bytes)
            {
                return Err("source kind/bounds".into());
            }
            if let Some(id) = &s.credential_ref {
                identifier(id)?;
            }
        }
        for d in &self.definitions {
            if d.version == 0
                || !["pipeline", "watch"].contains(&d.kind.as_str())
                || !["preserve", "reset", "migrate"].contains(&d.parameter_change.as_str())
            {
                return Err("monitoring definition kind/version/state policy".into());
            }
            value::validate_schema(&d.state_schema)?;
            if !value::matches_schema(&d.initial_state, &d.state_schema) {
                return Err("initial state schema".into());
            }
            let s = Script::new()?;
            let method = if d.kind == "pipeline" {
                "run"
            } else {
                "evaluate"
            };
            s.eval(&format!("globalThis.definition=({});if(typeof definition.{method} !== 'function')throw Error('entry point required')",d.source))?;
            if let Some(m) = &d.migrate {
                s.eval(&format!(
                    "if(typeof ({m})!=='function')throw Error('migration required')"
                ))?;
            }
            if d.parameter_change == "migrate" && d.migrate.is_none() {
                return Err("migration required".into());
            }
        }
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for v in store.instances()? {
            graph.insert(
                format!("v:{}", v.id),
                store
                    .definition(&v.definition)?
                    .dependencies
                    .iter()
                    .map(|x| format!("v:{x}"))
                    .collect(),
            );
        }
        let mut producers = HashSet::new();
        for i in &self.instances {
            let d = self.definition(&i.definition)?;
            value::validate(&i.params)?;
            if !(1..=300_000).contains(&i.timeout_ms)
                || i.retries > 8
                || i.retry_delay_ms > 300_000
                || i.retries > 0 && !i.repeat_safe
                || i.stale_after_ms == Some(0)
            {
                return Err("instance execution bounds/retry safety".into());
            }
            if let Some(s) = &i.schedule {
                s.validate()?;
            }
            for bindings in [&i.inputs, &i.outputs, &i.sources, &i.actions] {
                if bindings.len() > 64 {
                    return Err("binding capacity".into());
                }
                for alias in bindings.keys() {
                    identifier(alias)?;
                }
            }
            if d.kind == "watch"
                && (!i.outputs.is_empty() || !i.sources.is_empty() || i.retries > 0)
            {
                return Err(
                    "Watches read inputs and request actions; use Pipelines for collection".into(),
                );
            }
            if d.kind == "pipeline" && !i.actions.is_empty() {
                return Err("Pipeline actions must be requested by Watches".into());
            }
            for src in i.sources.values() {
                self.source(src)?;
            }
            let key = format!("m:{}", i.id);
            let edges = graph.entry(key.clone()).or_default();
            for input in i.inputs.values() {
                store.instance(input)?;
                edges.push(format!("v:{input}"));
            }
            for output in i.outputs.values() {
                let v = store.instance(output)?;
                if store.definition(&v.definition)?.kind != "stored" || !producers.insert(output) {
                    return Err("output requires a uniquely owned stored variable".into());
                }
                graph
                    .entry(format!("v:{output}"))
                    .or_default()
                    .push(key.clone());
            }
            for action in i.actions.values() {
                if self.definition(&self.instance(action)?.definition)?.kind != "pipeline" {
                    return Err("Watch action must target Pipeline".into());
                }
                graph
                    .entry(format!("m:{action}"))
                    .or_default()
                    .push(key.clone());
            }
        }
        fn visit(
            id: &str,
            g: &HashMap<String, Vec<String>>,
            active: &mut HashSet<String>,
            done: &mut HashSet<String>,
        ) -> Result<()> {
            if done.contains(id) {
                return Ok(());
            }
            if !active.insert(id.into()) {
                return Err("monitoring dependency cycle".into());
            }
            for dep in g.get(id).into_iter().flatten() {
                visit(dep, g, active, done)?;
            }
            active.remove(id);
            done.insert(id.into());
            Ok(())
        }
        let mut done = HashSet::new();
        for id in graph.keys() {
            visit(id, &graph, &mut HashSet::new(), &mut done)?;
        }
        Ok(())
    }
}
impl Store {
    pub fn monitoring_config(&self) -> Result<MonitoringConfig> {
        let body: Option<String> = self
            .conn
            .query_row("SELECT body FROM monitoring_config WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        body.map(|s| serde_json::from_str(&s).map_err(err))
            .unwrap_or_else(|| Ok(MonitoringConfig::default()))
    }
    pub fn monitor_state(&self, id: &str) -> Result<MonitorState> {
        let s: String = self
            .conn
            .query_row("SELECT body FROM monitoring_state WHERE id=?", [id], |r| {
                r.get(0)
            })
            .map_err(err)?;
        serde_json::from_str(&s).map_err(err)
    }
    pub fn configure_monitoring(&mut self, next: &MonitoringConfig, expected: u64) -> Result<()> {
        let old = self.monitoring_config()?;
        if old.version != expected || next.version != expected + 1 {
            return Err("monitoring configuration conflict".into());
        }
        next.validate(self)?;
        for d in &next.definitions {
            if let Ok(od) = old.definition(&d.id) {
                if od != d && d.version != od.version + 1 {
                    return Err("definition version conflict".into());
                }
            } else if d.version != 1 {
                return Err("initial definition version must be one".into());
            }
        }
        let mut states = vec![];
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
        for i in &next.instances {
            let d = next.definition(&i.definition)?;
            if let Ok(od) = old.definition(&d.id) {
                if (od != d && d.version != od.version + 1) || (od == d && d.version != od.version)
                {
                    return Err("definition version conflict".into());
                }
            }
            let prior = old.instance(&i.id).ok();
            let mut st = if prior.is_some() {
                self.monitor_state(&i.id)?
            } else {
                MonitorState {
                    id: i.id.clone(),
                    generation: 1,
                    revision: 1,
                    state: d.initial_state.clone(),
                    error: None,
                    next_due: None,
                    last_due: None,
                    missed: 0,
                    observed: BTreeMap::new(),
                }
            };
            if let Some(p) = prior {
                let changed = p != i
                    || old.definition(&p.definition)? != d
                    || i.sources
                        .values()
                        .any(|s| old.source(s).ok() != next.source(s).ok());
                if changed {
                    let parameters = p.params != i.params
                        || p.inputs != i.inputs
                        || p.outputs != i.outputs
                        || p.sources != i.sources
                        || p.actions != i.actions
                        || p.definition != i.definition;
                    if parameters && d.parameter_change == "reset" {
                        st.state = d.initial_state.clone();
                    } else if let Some(m) = &d.migrate {
                        let mut s = Script::new()?;
                        s.deadline = Some(deadline);
                        st.state=serde_json::from_str(&s.string(&format!("TaliaValue.stringify(({m})(TaliaValue.decode({}),TaliaValue.decode({}),TaliaValue.decode({})))",st.state,i.params,p.params))?).map_err(err)?;
                    }
                    st.generation += 1;
                    st.revision += 1;
                    st.error = None;
                    st.observed.clear();
                    if p.schedule != i.schedule || p.enabled != i.enabled {
                        st.next_due = None;
                        st.last_due = None;
                    }
                }
            }
            if !value::matches_schema(&st.state, &d.state_schema) {
                return Err("monitoring state migration schema".into());
            }
            states.push(st);
        }
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute("INSERT INTO monitoring_config VALUES(1,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",[serde_json::to_string(next).map_err(err)?]).map_err(err)?;
        tx.execute("DELETE FROM monitoring_state", [])
            .map_err(err)?;
        for s in states {
            tx.execute(
                "INSERT INTO monitoring_state VALUES(?,?)",
                params![s.id, serde_json::to_string(&s).map_err(err)?],
            )
            .map_err(err)?;
        }
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])
            .map_err(err)?;
        tx.commit().map_err(err)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn config() -> MonitoringConfig {
        serde_json::from_value(json!({"version":1,"definitions":[{"id":"collect","version":1,"kind":"pipeline","source":"{async run(ctx){}}"}],"instances":[{"id":"x","definition":"collect","outputs":{"metric":"metric"}},{"id":"y","definition":"collect"}]})).unwrap()
    }
    #[test]
    fn atomic_updates_and_independent_state() {
        let mut s = Store::open(":memory:").unwrap();
        crate::store::tests::seed(&mut s);
        let mut c = config();
        s.configure_monitoring(&c, 0).unwrap();
        let original = s.monitor_state("x").unwrap();
        c.version = 2;
        c.definitions[0].version = 2;
        c.definitions[0].migrate = Some(
            "(state,p)=>{if(p===2)throw Error('bad migration'); return {migrated:true}}".into(),
        );
        c.instances[1].params = value::number(2.0);
        assert!(s.configure_monitoring(&c, 1).is_err());
        assert_eq!(s.monitoring_config().unwrap().version, 1);
        assert_eq!(s.monitor_state("x").unwrap().state, original.state);
        c.instances[1].params = value::number(3.0);
        s.configure_monitoring(&c, 1).unwrap();
        assert_eq!(s.monitor_state("x").unwrap().generation, 2);
        c.version = 3;
        c.instances[0].params = value::number(9.0);
        s.configure_monitoring(&c, 2).unwrap();
        assert_eq!(s.monitor_state("y").unwrap().generation, 2);
        assert_eq!(s.monitor_state("x").unwrap().generation, 3);
        c.version = 4;
        c.instances[0]
            .inputs
            .insert("cycle".into(), "metric".into());
        assert!(s.configure_monitoring(&c, 3).is_err());
        assert_eq!(s.monitoring_config().unwrap().version, 3);
        assert!(s.configure_monitoring(&config(), 0).is_err());
    }
    #[test]
    fn sqlite_v1_upgrade_preserves_existing_values() {
        let path = std::env::temp_dir().join(format!("talia-migration-{}.db", std::process::id()));
        let mut s = Store::open(&path).unwrap();
        let original = crate::store::tests::seed(&mut s);
        s.conn
            .execute_batch(
                "DROP TABLE monitoring_config; DROP TABLE monitoring_state; PRAGMA user_version=1;",
            )
            .unwrap();
        drop(s);
        let mut s = Store::open(&path).unwrap();
        assert_eq!(s.instance("metric").unwrap(), original);
        s.configure_monitoring(&config(), 0).unwrap();
        drop(s);
        let s = Store::open(&path).unwrap();
        assert_eq!(s.monitoring_config().unwrap().version, 1);
        assert_eq!(s.instance("metric").unwrap(), original);
        drop(s);
        std::fs::remove_file(path).unwrap();
    }
}
