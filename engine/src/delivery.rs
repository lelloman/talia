//! Coherent saved packages and revisioned desired assignments. No live adoption on save.
use crate::{
    authority::{ErrorCode as Error, Result},
    catalog::{Change, ChangeSet, Key},
    clients::{secret, Host},
    definitions::identifier,
    store::Store,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Assignment {
    pub dashboard_id: String,
    #[serde(default = "object")]
    pub params: Value,
    #[serde(default = "object")]
    pub presentation: Value,
}
fn object() -> Value {
    json!({})
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Assigned {
    pub revision: u64,
    #[serde(flatten)]
    pub assignment: Assignment,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delivery {
    pub assignment: Assigned,
    pub package: Value,
}
impl Assignment {
    fn validate(&self) -> Result<()> {
        identifier(&self.dashboard_id).map_err(|_| Error::InvalidInput)?;
        if !self.params.is_object() || !self.presentation.is_object() {
            return Err(Error::InvalidInput);
        }
        if serde_json::to_vec(self)?.len() > 32768 {
            return Err(Error::LimitExceeded);
        }
        for (k, v) in self.presentation.as_object().unwrap() {
            match k.as_str() {
                "scale" if v.as_f64().is_some_and(|s| (0.25..=8.).contains(&s)) => (),
                _ => return Err(Error::InvalidInput),
            }
        }
        Ok(())
    }
}
impl Store {
    pub(crate) fn saved_package(&self, id: &str) -> Result<Value> {
        let body: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM dashboard_packages WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(serde_json::from_str(&body.ok_or(Error::NotFound)?)?)
    }
    pub fn assignment_get(&self, client: &str, slot: Option<&str>) -> Result<Assigned> {
        let row: Option<(u64, String)> = self
            .conn
            .query_row(
                "SELECT revision,body FROM dashboard_assignments WHERE client=? AND slot=?",
                params![client, slot.unwrap_or("")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (revision, body) = row.ok_or(Error::NotFound)?;
        Ok(Assigned {
            revision,
            assignment: serde_json::from_str(&body)?,
        })
    }
    /// Trusted operator/agent adapter API. The caller must independently authorize and audit assignment changes.
    pub fn assignment_set(
        &mut self,
        client: &str,
        slot: Option<&str>,
        expected: u64,
        assignment: &Assignment,
    ) -> Result<Assigned> {
        assignment.validate()?;
        let mut effective = self.saved_package(&assignment.dashboard_id)?;
        effective["params"]
            .as_object_mut()
            .ok_or(Error::InternalError)?
            .extend(
                assignment
                    .params
                    .as_object()
                    .ok_or(Error::InvalidInput)?
                    .clone(),
            );
        if serde_json::to_vec(&effective)?.len() > 262144 {
            return Err(Error::LimitExceeded);
        }
        let actual = self.assignment_get(client, slot)?;
        if actual.revision != expected {
            return Err(Error::Conflict);
        }
        let revision = expected
            .checked_add(1)
            .filter(|n| *n < 9_007_199_254_740_991)
            .ok_or(Error::LimitExceeded)?;
        self.conn.execute("UPDATE dashboard_assignments SET revision=?,body=? WHERE client=? AND slot=? AND revision=?",params![revision,serde_json::to_string(assignment)?,client,slot.unwrap_or(""),expected])?;
        Ok(Assigned {
            revision,
            assignment: assignment.clone(),
        })
    }
    fn delivery_owner(&self, host: &Host, slot: &str, owner: &str) -> Result<()> {
        identifier(slot).map_err(|_| Error::InvalidInput)?;
        let digest = secret(owner)?;
        let stored: Option<String> = self
            .conn
            .query_row(
                "SELECT owner FROM dashboard_assignments WHERE client=? AND slot=?",
                params![host.id(), slot],
                |r| r.get(0),
            )
            .optional()?;
        if stored.as_deref() != Some(&digest) {
            return Err(Error::Forbidden);
        }
        Ok(())
    }
    pub fn delivery_open(&mut self, host: &Host, slot: &str, owner: &str) -> Result<Assigned> {
        identifier(slot).map_err(|_| Error::InvalidInput)?;
        let digest = secret(owner)?;
        match self.assignment_get(host.id(), Some(slot)) {
            Ok(a) => {
                self.delivery_owner(host, slot, owner)?;
                return Ok(a);
            }
            Err(Error::NotFound) => (),
            Err(e) => return Err(e),
        }
        // A slot already registered through the older host protocol retains its original owner.
        let previous: Option<String> = self
            .conn
            .query_row(
                "SELECT owner FROM dashboard_slots WHERE client=? AND id=?",
                params![host.id(), slot],
                |r| r.get(0),
            )
            .optional()?;
        if previous.as_ref().is_some_and(|s| s != &digest) {
            return Err(Error::Forbidden);
        }
        let n: u64 = self.conn.query_row(
            "SELECT count(*) FROM dashboard_assignments WHERE client=? AND slot!=''",
            [host.id()],
            |r| r.get(0),
        )?;
        if n >= 64 {
            return Err(Error::LimitExceeded);
        }
        let default = match self.assignment_get(host.id(), None) {
            Ok(a) => a.assignment,
            Err(Error::NotFound) => {
                let id:Option<String>=self.conn.query_row("SELECT id FROM dashboard_packages ORDER BY CASE WHEN id='monitor' THEN 0 ELSE 1 END,id LIMIT 1",[],|r|r.get(0)).optional()?;
                Assignment {
                    dashboard_id: id.ok_or(Error::NotFound)?,
                    params: object(),
                    presentation: object(),
                }
            }
            Err(e) => return Err(e),
        };
        let tx = self.conn.savepoint()?;
        let body = serde_json::to_string(&default)?;
        tx.execute(
            "INSERT OR IGNORE INTO dashboard_assignments VALUES(?,'',NULL,1,?)",
            params![host.id(), body],
        )?;
        tx.execute(
            "INSERT INTO dashboard_assignments VALUES(?,?,?,1,?)",
            params![host.id(), slot, digest, body],
        )?;
        tx.commit()?;
        self.assignment_get(host.id(), Some(slot))
    }
    pub fn delivery_select(
        &mut self,
        host: &Host,
        slot: &str,
        owner: &str,
        expected: u64,
        assignment: &Assignment,
    ) -> Result<Assigned> {
        self.delivery_owner(host, slot, owner)?;
        self.assignment_set(host.id(), Some(slot), expected, assignment)
    }
    pub fn delivery_prepare(&self, host: &Host, slot: &str, owner: &str) -> Result<Delivery> {
        self.delivery_owner(host, slot, owner)?;
        let assignment = self.assignment_get(host.id(), Some(slot))?;
        let mut package = self.saved_package(&assignment.assignment.dashboard_id)?;
        let params = package["params"]
            .as_object_mut()
            .ok_or(Error::InternalError)?;
        params.extend(
            assignment
                .assignment
                .params
                .as_object()
                .ok_or(Error::InternalError)?
                .clone(),
        );
        if serde_json::to_vec(&package)?.len() > 262144 {
            return Err(Error::LimitExceeded);
        }
        Ok(Delivery {
            assignment,
            package,
        })
    }
    pub fn delivery_confirm(
        &self,
        host: &Host,
        slot: &str,
        owner: &str,
        revision: u64,
    ) -> Result<()> {
        self.delivery_owner(host, slot, owner)?;
        let a = self.assignment_get(host.id(), Some(slot))?;
        if a.revision != revision {
            return Err(Error::Conflict);
        }
        self.saved_package(&a.assignment.dashboard_id)?;
        Ok(())
    }
    /// Source fixtures are bootstrapped into the same saved catalog once; never replace operator edits.
    pub fn seed_dashboards(&mut self) -> crate::store::Result<()> {
        let seeded: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM metadata WHERE key='dashboard_seeded')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if seeded {
            return Ok(());
        }
        let count: u64 = self
            .conn
            .query_row("SELECT count(*) FROM dashboard_packages", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if count > 0 {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO metadata VALUES('dashboard_seeded',1)",
                    [],
                )
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        let changes = vec![
            Change::Put {
                key: Key::new("ui", "notice"),
                document: json!({"source":include_str!("../../dashboard/examples/notice.ui")}),
                migration: None,
                initial: None,
            },
            Change::Put {
                key: Key::new("dashboard", "monitor"),
                document: json!({"ui":include_str!("../../dashboard/examples/monitor.ui"),"view_model":include_str!("../../dashboard/examples/monitor.vm.js"),"references":[{"kind":"ui","id":"notice"}],"params":{"sidebar":false},"grants":{"reads":["value"],"writes":["value"],"runs":[]}}),
                migration: None,
                initial: None,
            },
            Change::Put {
                key: Key::new("dashboard", "monitoring"),
                document: json!({"ui":include_str!("../../dashboard/examples/monitoring.ui"),"view_model":include_str!("../../dashboard/examples/monitoring.vm.js"),"grants":{"reads":["cpu","memory","disk","disk-y","breakdown","monitor.disk-x","monitor.disk-y","monitor.investigate","monitor.collection"],"writes":[],"runs":["investigate"]}}),
                migration: None,
                initial: None,
            },
        ];
        self.catalog_save(&ChangeSet {
            expected_catalog_revision: self.catalog_revision()?,
            changes,
        })
        .map_err(|e| e.message)?;
        self.conn
            .execute(
                "INSERT OR IGNORE INTO metadata VALUES('dashboard_seeded',1)",
                [],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
#[cfg(test)]
mod tests;
