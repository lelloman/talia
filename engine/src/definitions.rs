use crate::{
    script::Script,
    store::{Definition, Instance, Result, Store},
    value,
};
use rusqlite::params;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn identifier(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        Err("invalid identifier".into())
    } else {
        Ok(())
    }
}
pub fn validate(d: &Definition) -> Result<()> {
    identifier(&d.id)?;
    value::validate_schema(&d.value_schema)?;
    value::validate_schema(&d.state_schema)?;
    if !["stored", "computed"].contains(&d.kind.as_str())
        || !["shared", "independent"].contains(&d.read_policy.as_str())
        || d.dependencies.len() > 64
    {
        return Err("definition kind/read policy/dependencies".into());
    }
    let mut unique = HashSet::new();
    for id in &d.dependencies {
        identifier(id)?;
        if !unique.insert(id) {
            return Err("duplicate dependency".into());
        }
    }
    if d.kind == "computed" {
        let s = Script::new()?;
        s.eval(&format!("globalThis.definition=({});if(typeof definition.get!=='function'||definition.set!==undefined&&typeof definition.set!=='function')throw Error('getter/setter required');",d.source))?;
    }
    Ok(())
}
fn graph(defs: &[Definition], instances: &[Instance]) -> Result<()> {
    let defs: HashMap<_, _> = defs.iter().map(|d| (&d.id, d)).collect();
    let instances: HashMap<_, _> = instances.iter().map(|i| (&i.id, i)).collect();
    fn visit<'a>(
        id: &'a String,
        defs: &HashMap<&String, &Definition>,
        all: &HashMap<&'a String, &'a Instance>,
        active: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if !active.insert(id.clone()) {
            return Err("dependency cycle".into());
        }
        let i = all.get(id).ok_or("missing dependency instance")?;
        let d = defs.get(&i.definition).ok_or("missing definition")?;
        for dep in &d.dependencies {
            visit(dep, defs, all, active, done)?;
        }
        active.remove(id);
        done.insert(id.clone());
        Ok(())
    }
    for id in instances.keys() {
        visit(
            id,
            &defs,
            &instances,
            &mut HashSet::new(),
            &mut HashSet::new(),
        )?;
    }
    Ok(())
}
impl Store {
    pub fn define(&mut self, d: &Definition, expected: u64, migration: Option<&str>) -> Result<()> {
        validate(d)?;
        let old = self.definition(&d.id).ok();
        if old.as_ref().map_or(0, |x| x.version) != expected || d.version != expected + 1 {
            return Err("definition conflict".into());
        }
        let mut defs = self.definitions()?;
        defs.retain(|x| x.id != d.id);
        defs.push(d.clone());
        let mut instances = self.instances()?;
        if defs.len() > 256 || instances.len() > 1024 {
            return Err("configuration capacity".into());
        }
        graph(&defs, &instances)?;
        let script = if let Some(source) = migration {
            let mut s = Script::new()?;
            s.deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(100));
            s.eval(&format!("globalThis.migrate=({source});if(typeof migrate!=='function')throw Error('migration function required')"))?;
            Some(s)
        } else {
            None
        };
        for i in instances.iter_mut().filter(|i| i.definition == d.id) {
            if let Some(s) = &script {
                let state = serde_json::to_string(&i.state).map_err(err)?;
                let params = serde_json::to_string(&i.params).map_err(err)?;
                // Migration is synchronous and capability-free; promises are unsupported state.
                let wire=s.string(&format!("TaliaValue.stringify(migrate(TaliaValue.decode({state}),TaliaValue.decode({params})))"))?;
                i.state = serde_json::from_str(&wire).map_err(err)?;
            }
            if !value::matches_schema(&i.state, &d.state_schema)
                || i.has_value && !value::matches_schema(&i.value, &d.value_schema)
            {
                return Err("migration schema mismatch".into());
            }
            i.generation += 1;
            i.revision += 1;
        }
        // This synchronous method holds engine ownership across staging and commit, but never I/O.
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO definitions VALUES(?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
            params![d.id, serde_json::to_string(d).map_err(err)?],
        )
        .map_err(err)?;
        for i in instances.iter().filter(|i| i.definition == d.id) {
            tx.execute(
                "UPDATE instances SET body=? WHERE id=?",
                params![serde_json::to_string(i).map_err(err)?, i.id],
            )
            .map_err(err)?;
        }
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(())
    }
    pub fn add_instance(&mut self, i: &Instance) -> Result<()> {
        identifier(&i.id)?;
        if i.revision != 1 || i.generation != 1 {
            return Err("initial revision/generation".into());
        }
        let mut all = self.instances()?;
        if all.len() >= 1024 {
            return Err("instance capacity".into());
        }
        all.push(i.clone());
        graph(&self.definitions()?, &all)?;
        self.create_instance(i)
    }
    pub fn reconfigure(&mut self, id: &str, expected: u64, parameters: Value) -> Result<()> {
        value::validate(&parameters)?;
        let mut i = self.instance(id)?;
        if i.revision != expected {
            return Err("conflict".into());
        }
        i.params = parameters;
        i.revision += 1;
        i.generation += 1;
        self.check_instance(&i)?;
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute(
            "UPDATE instances SET body=? WHERE id=?",
            params![serde_json::to_string(&i).map_err(err)?, id],
        )
        .map_err(err)?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(())
    }
    pub fn remove_instance(&mut self, id: &str) -> Result<()> {
        let mut all = self.instances()?;
        all.retain(|i| i.id != id);
        graph(&self.definitions()?, &all)?;
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM instances WHERE id=?", [id])
            .map_err(err)?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(())
    }
    pub fn remove_definition(&mut self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM definitions WHERE id=?", [id])
            .map_err(err)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_activation_is_all_or_nothing() {
        let mut s = Store::open(":memory:").unwrap();
        let x = crate::store::tests::seed(&mut s);
        let mut y = x.clone();
        y.id = "other".into();
        y.params = value::number(2.0);
        s.add_instance(&y).unwrap();
        let mut d = s.definition("stored").unwrap();
        d.version = 2;
        d.state_schema = "number".into();
        assert!(s
            .define(
                &d,
                1,
                Some("(state,p)=>{if(p===2)throw Error('bad');return 7}")
            )
            .is_err());
        assert_eq!(s.definition("stored").unwrap().version, 1);
        assert_eq!(s.instance("metric").unwrap(), x);
        s.define(&d, 1, Some("()=>7")).unwrap();
        assert_eq!(
            s.instance("other").unwrap().state["value"][1].as_f64(),
            Some(7.0)
        );
        let mut late = x.clone();
        late.revision += 1;
        assert!(s.commit(1, &late, false, 100).is_err());
        s.reconfigure("metric", 2, value::number(9.0)).unwrap();
        assert_eq!(s.instance("other").unwrap().params, y.params);
        assert!(s.remove_definition("stored").is_err());
    }
    #[test]
    fn invalid_code_cycle_and_budget() {
        let mut s = Store::open(":memory:").unwrap();
        crate::store::tests::seed(&mut s);
        let mut d = s.definition("stored").unwrap();
        d.version = 2;
        d.dependencies = vec!["metric".into()];
        assert!(s.define(&d, 1, None).is_err());
        d.dependencies.clear();
        d.kind = "computed".into();
        d.source = "{get:".into();
        assert!(s.define(&d, 1, None).is_err());
        d.source = "{get(){return 1}}".into();
        assert!(s.define(&d, 1, Some("()=>{while(true){}}")).is_err());
        assert_eq!(s.definition("stored").unwrap().version, 1);
    }
}
