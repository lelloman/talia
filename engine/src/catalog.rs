//! Authored source catalog. Engine records are projections of the canonical P2/P3 tables.
use crate::{
    definitions,
    monitoring::{DataSource, MonitorDefinition, MonitorInstance, MonitoringConfig},
    script::Script,
    store::{Definition, Instance, Store},
    value,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub kind: String,
    pub id: String,
}
impl Key {
    pub fn new(kind: &str, id: &str) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Initial {
    pub state: Value,
    pub value: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Put {
        key: Key,
        document: Value,
        #[serde(default)]
        migration: Option<String>,
        #[serde(default)]
        initial: Option<Initial>,
    },
    Delete {
        key: Key,
    },
}
impl Change {
    fn key(&self) -> &Key {
        match self {
            Self::Put { key, .. } | Self::Delete { key } => key,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeSet {
    pub expected_catalog_revision: u64,
    pub changes: Vec<Change>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub key: Key,
    pub revision: String,
    pub document: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub catalog_revision: u64,
    pub records: Vec<Record>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub catalog_revision: u64,
    pub changed: Vec<Key>,
    pub packages: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    #[serde(skip)]
    pub(crate) source_diagnostic: bool,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<Key>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_revision: Option<u64>,
}
impl Diagnostic {
    fn new(code: &str, message: impl ToString) -> Self {
        Self {
            source_diagnostic: false,
            code: code.into(),
            message: message.to_string(),
            key: None,
            path: None,
            line: None,
            column: None,
            current_revision: None,
        }
    }
    fn at(mut self, key: &Key, path: &str) -> Self {
        self.key = Some(key.clone());
        self.path = Some(path.into());
        self
    }
}
impl From<String> for Diagnostic {
    fn from(e: String) -> Self {
        Self::new("validation_failed", e)
    }
}
impl From<rusqlite::Error> for Diagnostic {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("storage_error", e)
    }
}
impl From<serde_json::Error> for Diagnostic {
    fn from(e: serde_json::Error) -> Self {
        Self::new("invalid_input", e)
    }
}
type Result<T> = std::result::Result<T, Diagnostic>;
#[derive(Debug, Serialize)]
pub struct Validation {
    pub valid: bool,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Variable {
    id: String,
    definition: String,
    params: Value,
    history_count: u32,
    history_age_ms: i64,
}
impl From<&Instance> for Variable {
    fn from(i: &Instance) -> Self {
        Self {
            id: i.id.clone(),
            definition: i.definition.clone(),
            params: i.params.clone(),
            history_count: i.history_count,
            history_age_ms: i.history_age_ms,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    source: String,
    #[serde(default)]
    references: Vec<Key>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Dashboard {
    ui: String,
    view_model: String,
    #[serde(default)]
    references: Vec<Key>,
    #[serde(default = "object")]
    params: Value,
    #[serde(default)]
    grants: Option<Value>,
}
fn object() -> Value {
    json!({})
}
fn authored(kind: &str) -> bool {
    matches!(kind, "dashboard" | "ui" | "vm" | "function")
}
fn known(kind: &str) -> bool {
    authored(kind)
        || matches!(
            kind,
            "variable_definition"
                | "variable"
                | "data_source"
                | "monitor_definition"
                | "monitor_instance"
        )
}
fn parse<T: serde::de::DeserializeOwned>(key: &Key, v: &Value) -> Result<T> {
    serde_json::from_value(v.clone()).map_err(|e| {
        let mut diagnostic = Diagnostic::from(e).at(key, "document");
        diagnostic.source_diagnostic = true; // Schema parsing reads only this document, never runtime state.
        diagnostic
    })
}
fn check_key(k: &Key) -> Result<()> {
    definitions::identifier(&k.id)?;
    if !known(&k.kind) || ["__proto__", "constructor", "prototype"].contains(&k.id.as_str()) {
        return Err(Diagnostic::new("invalid_input", "invalid catalog key").at(k, "key"));
    }
    Ok(())
}
impl Store {
    pub fn catalog_revision(&self) -> crate::store::Result<u64> {
        self.conn
            .query_row(
                "SELECT value FROM metadata WHERE key='catalog_revision'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }
    pub fn catalog_snapshot(&self) -> Result<Snapshot> {
        let revision = self.catalog_revision()?;
        let mut records = vec![];
        let mut q = self
            .conn
            .prepare("SELECT kind,id,revision,body FROM authored_definitions ORDER BY kind,id")?;
        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })? {
            let (kind, id, rev, body) = row?;
            records.push(Record {
                key: Key { kind, id },
                revision: rev.to_string(),
                document: serde_json::from_str(&body)?,
            });
        }
        let mut add = |kind: &str, id: &str, document: Value| {
            records.push(Record {
                key: Key::new(kind, id),
                revision: revision.to_string(),
                document,
            })
        };
        for d in self.definitions()? {
            add("variable_definition", &d.id, serde_json::to_value(&d)?);
        }
        for i in self.instances()? {
            add("variable", &i.id, serde_json::to_value(Variable::from(&i))?);
        }
        let c = self.monitoring_config()?;
        for s in c.sources {
            add("data_source", &s.id, serde_json::to_value(&s)?);
        }
        for d in c.definitions {
            add("monitor_definition", &d.id, serde_json::to_value(&d)?);
        }
        for i in c.instances {
            add("monitor_instance", &i.id, serde_json::to_value(&i)?);
        }
        records.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(Snapshot {
            catalog_revision: revision,
            records,
        })
    }
    pub fn dashboard_package(&self, id: &str) -> crate::store::Result<Value> {
        let body: String = self
            .conn
            .query_row(
                "SELECT body FROM dashboard_packages WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&body).map_err(|e| e.to_string())
    }
    /// Runs the exact activation path in a rollback-only savepoint, including bounded migrations.
    pub fn catalog_validate(&mut self, set: &ChangeSet) -> Validation {
        match self.catalog_transaction(set, false) {
            Ok(_) => Validation {
                valid: true,
                diagnostics: vec![],
            },
            Err(e) => Validation {
                valid: false,
                diagnostics: vec![e],
            },
        }
    }
    pub fn catalog_save(&mut self, set: &ChangeSet) -> Result<Receipt> {
        self.catalog_transaction(set, true)
    }
    fn catalog_transaction(&mut self, set: &ChangeSet, commit: bool) -> Result<Receipt> {
        self.conn.execute_batch("SAVEPOINT catalog_batch")?;
        let result = self.catalog_apply(set);
        if result.is_ok() && commit {
            self.conn.execute_batch("RELEASE catalog_batch")?;
        } else {
            self.conn
                .execute_batch("ROLLBACK TO catalog_batch; RELEASE catalog_batch")?;
        }
        result
    }
    fn catalog_apply(&mut self, set: &ChangeSet) -> Result<Receipt> {
        let old = self.catalog_snapshot()?;
        if old.catalog_revision != set.expected_catalog_revision {
            let mut e = Diagnostic::new("conflict", "catalog revision changed");
            e.current_revision = Some(old.catalog_revision);
            return Err(e);
        }
        if set.changes.is_empty()
            || set.changes.len() > 256
            || serde_json::to_vec(set)?.len() > 2_097_152
        {
            return Err(Diagnostic::new("limit_exceeded", "change set bounds"));
        }
        let before: BTreeMap<_, _> = old
            .records
            .into_iter()
            .map(|r| (r.key, r.document))
            .collect();
        let mut after = before.clone();
        let mut keys = BTreeSet::new();
        for change in &set.changes {
            let key = change.key();
            check_key(key)?;
            if !keys.insert(key.clone()) {
                return Err(Diagnostic::new("invalid_input", "duplicate change key").at(key, "key"));
            }
            match change {
                Change::Put {
                    document,
                    migration,
                    initial,
                    ..
                } => {
                    if migration.is_some() && key.kind != "variable_definition"
                        || initial.is_some() && (key.kind != "variable" || before.contains_key(key))
                    {
                        return Err(Diagnostic::new(
                            "invalid_input",
                            "migration is definition-only; initial is new-variable-only",
                        )
                        .at(key, "document"));
                    }
                    after.insert(key.clone(), document.clone());
                }
                Change::Delete { .. } => {
                    if after.remove(key).is_none() {
                        return Err(
                            Diagnostic::new("not_found", "definition missing").at(key, "key")
                        );
                    }
                }
            }
        }
        if after.len() > 2048
            || serde_json::to_vec(&after.values().collect::<Vec<_>>())?.len() > 8_388_608
        {
            return Err(Diagnostic::new("limit_exceeded", "catalog capacity"));
        }
        self.catalog_engine(&before, &after, &set.changes)?;
        // Runtime samples do not advance this counter. One bundle may advance it more than once.
        self.conn.execute(
            "UPDATE metadata SET value=value+1 WHERE key='catalog_revision'",
            [],
        )?;
        let revision = self.catalog_revision()?;
        let packages = compile_packages(&after, revision)?;
        let mut changed_packages = BTreeMap::new();
        let old_packages: Vec<String> = {
            let mut q = self.conn.prepare("SELECT id FROM dashboard_packages")?;
            let rows = q.query_map([], |r| r.get(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        for id in old_packages {
            if !packages.contains_key(&id) {
                self.conn
                    .execute("DELETE FROM dashboard_packages WHERE id=?", [id])?;
            }
        }
        for (id, mut pkg) in packages {
            let old = self.dashboard_package(&id).ok();
            if let Some(mut previous) = old {
                previous["revision"] = pkg["revision"].clone();
                if previous == pkg {
                    continue;
                }
            }
            pkg["revision"] = json!(format!("catalog-{revision}-{id}"));
            changed_packages.insert(id.clone(), pkg["revision"].as_str().unwrap().into());
            self.conn.execute("INSERT INTO dashboard_packages VALUES(?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![id,pkg.to_string()])?;
        }
        let changed: Vec<Key> = keys
            .into_iter()
            .filter(|k| before.get(k) != after.get(k))
            .collect();
        for key in changed.iter().filter(|k| authored(&k.kind)) {
            if let Some(doc) = after.get(key) {
                self.conn.execute("INSERT INTO authored_definitions VALUES(?,?,?,?) ON CONFLICT(kind,id) DO UPDATE SET revision=excluded.revision,body=excluded.body",params![key.kind,key.id,revision,doc.to_string()])?;
            } else {
                self.conn.execute(
                    "DELETE FROM authored_definitions WHERE kind=? AND id=?",
                    params![key.kind, key.id],
                )?;
            }
        }
        Ok(Receipt {
            catalog_revision: revision,
            changed,
            packages: changed_packages,
        })
    }
    fn catalog_engine(
        &mut self,
        before: &BTreeMap<Key, Value>,
        after: &BTreeMap<Key, Value>,
        changes: &[Change],
    ) -> Result<()> {
        let mut defs = Vec::<Definition>::new();
        let mut vars = Vec::<Variable>::new();
        let old_monitor = self.monitoring_config()?;
        let mut next = MonitoringConfig {
            version: old_monitor.version,
            sources: vec![],
            definitions: vec![],
            instances: vec![],
        };
        for (key, doc) in after.iter().filter(|(k, _)| !authored(&k.kind)) {
            if doc["id"].as_str() != Some(&key.id) {
                return Err(
                    Diagnostic::new("invalid_input", "document ID must match key")
                        .at(key, "document.id"),
                );
            }
            match key.kind.as_str() {
                "variable_definition" => {
                    let d: Definition = parse(key, doc)?;
                    definitions::validate(&d)
                        .map_err(|e| Diagnostic::from(e).at(key, "document"))?;
                    if before.get(key) != Some(doc) {
                        let expected = self.definition(&key.id).ok().map_or(0, |x| x.version);
                        if d.version != expected + 1 {
                            return Err(Diagnostic::new(
                                "conflict",
                                "definition version must increment",
                            )
                            .at(key, "document.version"));
                        }
                    }
                    defs.push(d);
                }
                "variable" => vars.push(parse(key, doc)?),
                "data_source" => next.sources.push(parse::<DataSource>(key, doc)?),
                "monitor_definition" => {
                    next.definitions.push(parse::<MonitorDefinition>(key, doc)?)
                }
                "monitor_instance" => next.instances.push(parse::<MonitorInstance>(key, doc)?),
                _ => {
                    return Err(
                        Diagnostic::new("invalid_input", "unknown record kind").at(key, "key.kind")
                    )
                }
            }
        }
        if defs.len() > 256 || vars.len() > 1024 {
            return Err(Diagnostic::new(
                "limit_exceeded",
                "engine configuration capacity",
            ));
        }
        let old_instances: BTreeMap<_, _> = self
            .instances()?
            .into_iter()
            .map(|i| (i.id.clone(), i))
            .collect();
        let mut instances = vec![];
        let new_generation = self
            .catalog_revision()?
            .checked_add(1)
            .ok_or_else(|| Diagnostic::new("limit_exceeded", "catalog revision exhausted"))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
        let mut migrations = BTreeMap::new();
        for c in changes {
            if let Change::Put {
                key,
                migration: Some(source),
                ..
            } = c
            {
                if before.get(key) == after.get(key) {
                    return Err(Diagnostic::new(
                        "invalid_input",
                        "migration requires a changed definition",
                    )
                    .at(key, "migration"));
                }
                let mut script = Script::new()?;
                script.deadline = Some(deadline);
                script.eval(&format!("globalThis.migrate=({source});if(typeof migrate!=='function')throw Error('migration function required');")).map_err(|e|Diagnostic::from(e).at(key,"migration"))?;
                migrations.insert(key.id.clone(), script);
            }
        }
        for v in vars {
            let key = Key::new("variable", &v.id);
            let d = defs.iter().find(|d| d.id == v.definition).ok_or_else(|| {
                Diagnostic::new("validation_failed", "missing Variable definition")
                    .at(&key, "document.definition")
            })?;
            let prior = old_instances.get(&v.id);
            if prior.is_some_and(|i| i.definition != v.definition) {
                return Err(Diagnostic::new(
                    "validation_failed",
                    "existing Variable cannot change definition; remove/create separately",
                )
                .at(&key, "document.definition"));
            }
            let mut i = prior.cloned().unwrap_or_else(|| Instance {
                id: v.id.clone(),
                definition: v.definition.clone(),
                params: v.params.clone(),
                state: value::undefined(),
                value: value::undefined(),
                has_value: false,
                timestamp: 0,
                quality: "unknown".into(),
                revision: 1,
                generation: new_generation,
                history_count: v.history_count,
                history_age_ms: v.history_age_ms,
            });
            if prior.is_none() {
                for c in changes {
                    if let Change::Put {
                        key: k,
                        initial: Some(initial),
                        ..
                    } = c
                    {
                        if k == &key {
                            i.state = initial.state.clone();
                            if let Some(value) = &initial.value {
                                i.value = value.clone();
                                i.has_value = true;
                            }
                        }
                    }
                }
            }
            let definition_key = Key::new("variable_definition", &v.definition);
            let changed = before.get(&key) != after.get(&key)
                || before.get(&definition_key) != after.get(&definition_key);
            if prior.is_some() && changed {
                if let Some(s) = migrations.get(&v.definition) {
                    let wire=s.string(&format!("TaliaValue.stringify(migrate(TaliaValue.decode({}),TaliaValue.decode({})))",i.state,v.params)).map_err(|e|Diagnostic::from(e).at(&definition_key,"migration"))?;
                    i.state = serde_json::from_str(&wire)?;
                }
                i.generation += 1;
                i.revision += 1;
            }
            i.params = v.params;
            i.history_count = v.history_count;
            i.history_age_ms = v.history_age_ms;
            value::validate(&i.params)
                .map_err(|e| Diagnostic::from(e).at(&key, "document.params"))?;
            if !value::matches_schema(&i.state, &d.state_schema)
                || i.has_value && !value::matches_schema(&i.value, &d.value_schema)
                || i.history_count > 100000
                || i.history_age_ms < 0
            {
                return Err(
                    Diagnostic::new("validation_failed", "Variable schema/history bounds")
                        .at(&key, "document"),
                );
            }
            instances.push(i);
        }
        definitions::graph(&defs, &instances)?;
        // Stage the whole final graph first, so references do not depend on change order.
        for d in &defs {
            self.conn.execute("INSERT INTO definitions VALUES(?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![d.id,serde_json::to_string(d)?])?;
        }
        for i in old_instances.values() {
            if !instances.iter().any(|n| n.id == i.id) {
                self.conn
                    .execute("DELETE FROM instances WHERE id=?", [&i.id])?;
            }
        }
        for i in &instances {
            self.conn.execute("INSERT INTO instances VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET definition=excluded.definition,body=excluded.body",params![i.id,i.definition,serde_json::to_string(i)?])?;
        }
        for d in self.definitions()? {
            if !defs.iter().any(|n| n.id == d.id) {
                self.conn
                    .execute("DELETE FROM definitions WHERE id=?", [d.id])?;
            }
        }
        let mut previous = old_monitor.clone();
        previous.sources.sort_by(|a, b| a.id.cmp(&b.id));
        previous.definitions.sort_by(|a, b| a.id.cmp(&b.id));
        previous.instances.sort_by(|a, b| a.id.cmp(&b.id));
        if next != previous {
            next.version += 1;
            self.configure_monitoring(&next, old_monitor.version)?;
        } else {
            next.validate(self)?;
        }
        if before
            .iter()
            .filter(|(k, _)| !authored(&k.kind))
            .ne(after.iter().filter(|(k, _)| !authored(&k.kind)))
        {
            self.conn
                .execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        }
        Ok(())
    }
}

mod compiler;
use compiler::compile_packages;
#[cfg(test)]
mod tests;
