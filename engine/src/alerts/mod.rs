//! Durable server-owned alerts. Transactions are short and never span external I/O.
use crate::store::{Result, Store};
use rusqlite::{params, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub(crate) fn key(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        Err("invalid alert identity".into())
    } else {
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Acknowledgement {
    pub actor: String,
    pub at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Alert {
    pub key: String,
    pub occurrence: u64,
    pub revision: u64,
    pub active: bool,
    pub stage: String,
    pub severity: String,
    pub message: String,
    pub labels: BTreeMap<String, String>,
    pub opened_at: i64,
    pub updated_at: i64,
    pub stage_at: i64,
    pub resolved_at: Option<i64>,
    pub acknowledgement: Option<Acknowledgement>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub key: String,
    pub active: bool,
    pub stage: String,
    pub severity: String,
    pub message: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub reset_ack: bool,
}
impl Store {
    pub(crate) fn alert_get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<Option<T>> {
        let s: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM alert_entities WHERE kind=? AND id=?",
                params![kind, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        s.map(|s| serde_json::from_str(&s).map_err(err)).transpose()
    }
    pub(crate) fn alert_list<T: DeserializeOwned>(&self, kind: &str) -> Result<Vec<T>> {
        let mut q = self
            .conn
            .prepare("SELECT body FROM alert_entities WHERE kind=? ORDER BY id")
            .map_err(err)?;
        let rows = q
            .query_map([kind], |r| r.get::<_, String>(0))
            .map_err(err)?;
        rows.map(|r| serde_json::from_str(&r.map_err(err)?).map_err(err))
            .collect()
    }
    pub(crate) fn alert_put(&self, kind: &str, id: &str, body: &impl Serialize) -> Result<()> {
        let body = serde_json::to_string(body).map_err(err)?;
        if body.len() > 262144 {
            return Err("alert record size limit".into());
        }
        if self.alert_get::<Value>(kind, id)?.is_none() {
            let count: u64 = self
                .conn
                .query_row(
                    "SELECT count(*) FROM alert_entities WHERE kind=?",
                    [kind],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if count >= 10000 {
                return Err("alert record capacity".into());
            }
        }
        self.conn.execute("INSERT INTO alert_entities VALUES(?,?,?) ON CONFLICT(kind,id) DO UPDATE SET body=excluded.body",params![kind,id,body]).map_err(err)?;
        Ok(())
    }
    pub(crate) fn alert_atomic<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.conn
            .execute_batch("SAVEPOINT alert_change")
            .map_err(err)?;
        match f(self) {
            Ok(v) => {
                self.conn
                    .execute_batch("RELEASE alert_change")
                    .map_err(err)?;
                Ok(v)
            }
            Err(e) => {
                self.conn
                    .execute_batch("ROLLBACK TO alert_change; RELEASE alert_change")
                    .map_err(err)?;
                Err(e)
            }
        }
    }
    pub(crate) fn alert_audit(
        &self,
        actor: &str,
        op: &str,
        id: &str,
        at: i64,
        body: Value,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO alert_audit(at,actor,operation,entity,body) VALUES(?,?,?,?,?)",
                params![at, actor, op, id, body.to_string()],
            )
            .map_err(err)?;
        self.conn.execute("DELETE FROM alert_audit WHERE seq <= (SELECT COALESCE(max(seq),0)-10000 FROM alert_audit)",[]).map_err(err)?;
        Ok(())
    }
    pub fn alerts(&self) -> Result<Vec<Alert>> {
        self.alert_list("alert")
    }
    pub fn alert(&self, id: &str) -> Result<Alert> {
        self.alert_get("alert", id)?.ok_or("alert not found".into())
    }
    pub fn alert_history(&self, id: &str) -> Result<Vec<Alert>> {
        Ok(self
            .alert_list::<Alert>("occurrence")?
            .into_iter()
            .filter(|a| a.key == id)
            .collect())
    }
    pub fn alert_audit_history(&self, id: Option<&str>) -> Result<Vec<Value>> {
        let mut q=self.conn.prepare("SELECT seq,at,actor,operation,entity,body FROM alert_audit WHERE (? IS NULL OR entity=?) ORDER BY seq DESC LIMIT 200").map_err(err)?;
        let rows=q.query_map(params![id,id],|r|Ok(json!({"id":r.get::<_,u64>(0)?,"at":r.get::<_,i64>(1)?,"actor":r.get::<_,String>(2)?,"operation":r.get::<_,String>(3)?,"entity":r.get::<_,String>(4)?,"detail":serde_json::from_str::<Value>(&r.get::<_,String>(5)?).unwrap_or(Value::Null)}))).map_err(err)?;
        rows.map(|r| r.map_err(err)).collect()
    }
    pub fn alert_observe(
        &mut self,
        o: &Observation,
        expected: u64,
        actor: &str,
        now: i64,
    ) -> Result<Option<Alert>> {
        key(&o.key)?;
        key(&o.stage)?;
        key(&o.severity)?;
        if o.message.len() > 4096
            || o.labels.len() > 32
            || o.labels
                .iter()
                .any(|(k, v)| key(k).is_err() || key(v).is_err())
        {
            return Err("invalid alert observation".into());
        }
        self.alert_atomic(|s| {
            let old = s.alert_get::<Alert>("alert", &o.key)?;
            if old.as_ref().map_or(0, |a| a.revision) != expected {
                return Err("conflict".into());
            }
            if old.is_none() && !o.active {
                return Ok(None);
            }
            let fresh = old.as_ref().is_none_or(|a| !a.active && o.active);
            let mut a = if fresh {
                Alert {
                    key: o.key.clone(),
                    occurrence: old.as_ref().map_or(1, |a| a.occurrence + 1),
                    revision: expected,
                    active: true,
                    stage: o.stage.clone(),
                    severity: o.severity.clone(),
                    message: o.message.clone(),
                    labels: o.labels.clone(),
                    opened_at: now,
                    updated_at: now,
                    stage_at: now,
                    resolved_at: None,
                    acknowledgement: None,
                }
            } else {
                old.clone().unwrap()
            };
            if a.stage != o.stage {
                a.stage_at = now;
            }
            if o.reset_ack && o.active {
                a.acknowledgement = None;
            }
            if a.active && !o.active {
                a.resolved_at = Some(now);
            }
            a.active = o.active;
            a.stage = o.stage.clone();
            a.severity = o.severity.clone();
            a.message = o.message.clone();
            a.labels = o.labels.clone();
            if old.as_ref().is_some_and(|p| *p == a) {
                return Ok(Some(a));
            }
            a.revision += 1;
            a.updated_at = now;
            s.alert_put("alert", &a.key, &a)?;
            s.alert_put("occurrence", &format!("{}:{}", a.key, a.occurrence), &a)?;
            s.alert_audit(
                actor,
                if fresh {
                    "opened"
                } else if !a.active {
                    "resolved"
                } else {
                    "updated"
                },
                &a.key,
                now,
                json!({"occurrence":a.occurrence,"revision":a.revision}),
            )?;
            Ok(Some(a))
        })
    }
    pub fn alert_acknowledge(
        &mut self,
        id: &str,
        occurrence: u64,
        expected: u64,
        actor: &str,
        now: i64,
    ) -> Result<Alert> {
        self.alert_atomic(|s| {
            let mut a = s.alert(id)?;
            if a.occurrence != occurrence || a.revision != expected {
                return Err("conflict".into());
            }
            if !a.active {
                return Err("alert resolved".into());
            }
            if a.acknowledgement.is_some() {
                return Ok(a);
            }
            a.acknowledgement = Some(Acknowledgement {
                actor: actor.into(),
                at: now,
            });
            a.revision += 1;
            a.updated_at = now;
            s.alert_put("alert", id, &a)?;
            s.alert_put("occurrence", &format!("{}:{}", id, a.occurrence), &a)?;
            s.alert_audit(
                actor,
                "acknowledged",
                id,
                now,
                json!({"occurrence":occurrence,"revision":a.revision}),
            )?;
            Ok(a)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn o(active: bool) -> Observation {
        Observation {
            key: "disk:host:/var".into(),
            active,
            stage: "warning".into(),
            severity: "warning".into(),
            message: "disk low".into(),
            labels: BTreeMap::new(),
            reset_ack: false,
        }
    }
    #[test]
    fn occurrences_ack_conflicts_and_recovery() {
        let mut s = Store::open(":memory:").unwrap();
        assert!(s.alert_observe(&o(false), 0, "watch", 1).unwrap().is_none());
        let a = s.alert_observe(&o(true), 0, "watch", 2).unwrap().unwrap();
        assert_eq!(
            s.alert_observe(&o(true), a.revision, "watch", 3)
                .unwrap()
                .unwrap(),
            a
        );
        let a = s
            .alert_acknowledge(&a.key, 1, a.revision, "human", 4)
            .unwrap();
        assert!(a.active);
        assert!(s.alert_acknowledge(&a.key, 1, 1, "other", 5).is_err());
        let a = s
            .alert_observe(&o(false), a.revision, "watch", 6)
            .unwrap()
            .unwrap();
        assert!(!a.active);
        let a = s
            .alert_observe(&o(true), a.revision, "watch", 7)
            .unwrap()
            .unwrap();
        assert_eq!(a.occurrence, 2);
        assert!(a.acknowledgement.is_none());
        assert_eq!(s.alert_history(&a.key).unwrap().len(), 2);
        assert_eq!(s.alert_audit_history(None).unwrap().len(), 4);
    }
    #[test]
    fn reset_and_backup() {
        let mut s = Store::open(":memory:").unwrap();
        let a = s.alert_observe(&o(true), 0, "watch", 1).unwrap().unwrap();
        let a = s.alert_acknowledge(&a.key, 1, 1, "human", 2).unwrap();
        let mut obs = o(true);
        obs.stage = "critical".into();
        obs.reset_ack = true;
        let a = s
            .alert_observe(&obs, a.revision, "watch", 3)
            .unwrap()
            .unwrap();
        assert!(a.acknowledgement.is_none());
        let path = std::env::temp_dir().join(format!("talia-alert-{}.db", std::process::id()));
        s.backup(&path).unwrap();
        let restored = Store::open(&path).unwrap();
        assert_eq!(restored.alert(&a.key).unwrap(), a);
        drop(restored);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod permission_tests {
    use super::*;
    use crate::authority::{Action, ErrorCode, Family, Grant, Scope};
    #[test]
    fn separated_permissions_and_revocation() {
        let mut s = Store::open(":memory:").unwrap();
        s.agent_policy_set(
            "reader",
            0,
            true,
            &[Grant {
                family: Family::Alerts,
                actions: [Action::Read].into_iter().collect(),
                scope: Scope::All,
            }],
        )
        .unwrap();
        let token = s.agent_credential_issue("reader").unwrap();
        let session = s.agent_authenticate(&token).unwrap();
        s.alert_require(&session, Action::Read).unwrap();
        assert_eq!(
            s.alert_require(&session, Action::Acknowledge),
            Err(ErrorCode::Forbidden)
        );
        s.agent_credential_revoke(&token).unwrap();
        assert_eq!(
            s.alert_require(&session, Action::Read),
            Err(ErrorCode::Unauthenticated)
        );
    }
}

pub mod policy;

pub mod delivery;

pub mod providers;

pub mod api;
