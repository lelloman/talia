//! Dashboard-scoped browser authority. Identity comes exclusively from the OIDC host.
use crate::{
    authority::{ErrorCode as E, Result},
    delivery::Assignment,
    store::Store,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Access {
    pub dashboard_id: String,
    pub owner: String,
    pub public: bool,
    pub viewers: Vec<String>,
    pub expected_revision: u64,
    pub request_id: String,
}
fn subject(s: &str) -> Result<()> {
    if s.is_empty() || s.len() > 512 || s.chars().any(char::is_control) {
        Err(E::InvalidInput)
    } else {
        Ok(())
    }
}
impl Store {
    pub fn dashboard_access_list(&self) -> Result<Value> {
        let mut q=self.conn.prepare("SELECT dashboard,owner,public,viewers,revision FROM dashboard_access ORDER BY dashboard")?;
        let rows=q.query_map([],|r|Ok(json!({"dashboardId":r.get::<_,String>(0)?,"owner":r.get::<_,String>(1)?,"public":r.get::<_,bool>(2)?,"viewers":serde_json::from_str::<Value>(&r.get::<_,String>(3)?).unwrap_or(json!([])),"revision":r.get::<_,u64>(4)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
        let mut q = self
            .conn
            .prepare("SELECT subject,name,admin FROM dashboard_users ORDER BY subject")?;
        let users=q.query_map([],|r|Ok(json!({"subject":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"admin":r.get::<_,bool>(2)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(json!({"dashboards":rows,"users":users}))
    }
    pub fn user_seen(&mut self, id: &str, name: &str) -> Result<()> {
        subject(id)?;
        if name.len() > 512 {
            return Err(E::InvalidInput);
        }
        self.conn.execute("INSERT INTO dashboard_users(subject,name) VALUES(?,?) ON CONFLICT(subject) DO UPDATE SET name=excluded.name",params![id,name])?;
        Ok(())
    }
    /// Explicit operator bootstrap; never infer administration from first login or a display name.
    pub fn user_bootstrap(&mut self, id: &str) -> Result<()> {
        subject(id)?;
        self.conn.execute("INSERT INTO dashboard_users(subject,name,admin) VALUES(?,?,1) ON CONFLICT(subject) DO NOTHING",params![id,id])?;
        Ok(())
    }
    pub fn user_admin(&self, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT admin FROM dashboard_users WHERE subject=?",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }
    pub fn user_can_open(&self, id: &str, dashboard: &str) -> Result<bool> {
        if self.user_admin(id)? {
            return Ok(true);
        }
        let a: Option<(String, bool, String)> = self
            .conn
            .query_row(
                "SELECT owner,public,viewers FROM dashboard_access WHERE dashboard=?",
                [dashboard],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        Ok(a.is_some_and(|(owner, public, viewers)| {
            owner == id
                || public
                || serde_json::from_str::<Vec<String>>(&viewers)
                    .unwrap_or_default()
                    .iter()
                    .any(|s| s == id)
        }))
    }
    pub fn user_package(&self, id: &str, dashboard: &str) -> Result<Value> {
        if !self.user_can_open(id, dashboard)? {
            return Err(E::Forbidden);
        }
        self.saved_package(dashboard)
    }
    pub fn user_catalog(&self, id: &str) -> Result<Value> {
        let admin = self.user_admin(id)?;
        let mut q = self
            .conn
            .prepare("SELECT id FROM dashboard_packages ORDER BY id")?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut dashboards = vec![];
        for d in ids {
            if self.user_can_open(id, &d)? {
                let a:Option<(String,bool,String,u64)>=self.conn.query_row("SELECT owner,public,viewers,revision FROM dashboard_access WHERE dashboard=?",[&d],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
                let mut item = json!({"id":d});
                if admin {
                    item["access"]=a.map(|(o,p,v,r)|json!({"owner":o,"public":p,"viewers":serde_json::from_str::<Value>(&v).unwrap_or(json!([])),"revision":r})).unwrap_or(Value::Null);
                }
                dashboards.push(item);
            }
        }
        let default: Option<String> = self
            .conn
            .query_row(
                "SELECT default_dashboard FROM dashboard_users WHERE subject=?",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let default = default.filter(|d| dashboards.iter().any(|v| v["id"] == *d));
        Ok(json!({"admin":admin,"dashboards":dashboards,"defaultDashboard":default}))
    }
    pub fn user_default(&mut self, id: &str, dashboard: &str) -> Result<()> {
        self.user_package(id, dashboard)?;
        self.conn.execute(
            "UPDATE dashboard_users SET default_dashboard=? WHERE subject=?",
            params![dashboard, id],
        )?;
        Ok(())
    }
    /// Caller must establish trusted admin/operator authority. Every change is versioned and audited.
    pub fn dashboard_access_set(&mut self, actor: &str, a: &Access) -> Result<Value> {
        subject(&a.owner)?;
        crate::definitions::identifier(&a.request_id).map_err(|_| E::InvalidInput)?;
        if a.viewers.len() > 256 {
            return Err(E::LimitExceeded);
        }
        for v in &a.viewers {
            subject(v)?;
        }
        let body = serde_json::to_string(a)?;
        let old: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT body,outcome FROM dashboard_access_audit WHERE actor=? AND request=?",
                params![actor, a.request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((b, o)) = old {
            return if b == body {
                Ok(serde_json::from_str(&o)?)
            } else {
                Err(E::Conflict)
            };
        }
        self.saved_package(&a.dashboard_id)?;
        if !self.user_admin(&a.owner)? {
            return Err(E::Forbidden);
        }
        let rev: u64 = self
            .conn
            .query_row(
                "SELECT revision FROM dashboard_access WHERE dashboard=?",
                [&a.dashboard_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if rev != a.expected_revision {
            return Err(E::Conflict);
        }
        let next = rev
            .checked_add(1)
            .filter(|n| *n < 9_007_199_254_740_991)
            .ok_or(E::LimitExceeded)?;
        let result = json!({"revision":next});
        let tx = self.conn.savepoint()?;
        tx.execute("INSERT INTO dashboard_access VALUES(?,?,?,?,?) ON CONFLICT(dashboard) DO UPDATE SET owner=excluded.owner,public=excluded.public,viewers=excluded.viewers,revision=excluded.revision",params![a.dashboard_id,a.owner,a.public,serde_json::to_string(&a.viewers)?,next])?;
        tx.execute(
            "INSERT INTO dashboard_access_audit(actor,request,body,outcome) VALUES(?,?,?,?)",
            params![actor, a.request_id, body, result.to_string()],
        )?;
        tx.commit()?;
        Ok(result)
    }
    pub fn user_request(&mut self, id: &str, body: Value) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
        enum Request {
            Catalog,
            Default { dashboard: String },
            Share { access: Access },
            Users,
            Role { subject: String, admin: bool },
        }
        match serde_json::from_value(body)? {
            Request::Catalog => self.user_catalog(id),
            Request::Default { dashboard } => {
                self.user_default(id, &dashboard)?;
                self.user_catalog(id)
            }
            Request::Share { access } => {
                if !self.user_admin(id)? {
                    return Err(E::Forbidden);
                }
                self.dashboard_access_set(id, &access)
            }
            Request::Users => {
                if !self.user_admin(id)? {
                    return Err(E::Forbidden);
                }
                let mut q = self
                    .conn
                    .prepare("SELECT subject,name,admin FROM dashboard_users ORDER BY subject")?;
                let rows=q.query_map([],|r|Ok(json!({"subject":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"admin":r.get::<_,bool>(2)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
                Ok(json!(rows))
            }
            Request::Role {
                subject: target,
                admin,
            } => {
                if !self.user_admin(id)? || target == id {
                    return Err(E::Forbidden);
                }
                subject(&target)?;
                let tx = self.conn.savepoint()?;
                if tx.execute(
                    "UPDATE dashboard_users SET admin=? WHERE subject=?",
                    params![admin, target],
                )? != 1
                {
                    return Err(E::NotFound);
                }
                tx.execute("INSERT INTO dashboard_access_audit(actor,request,body,outcome) VALUES(?,lower(hex(randomblob(16))),?,?)",params![id,json!({"subject":target,"admin":admin}).to_string(),"{}"])?;
                tx.commit()?;
                Ok(Value::Null)
            }
        }
    }
    pub fn user_client_request(
        &mut self,
        id: &str,
        credential: &str,
        body: Value,
        now: i64,
    ) -> Result<Value> {
        self.conn.execute_batch("SAVEPOINT browser_client")?;
        match self.user_client_request_inner(id, credential, body, now) {
            Ok(value) => {
                self.conn.execute_batch("RELEASE browser_client")?;
                Ok(value)
            }
            Err(error) => {
                self.conn
                    .execute_batch("ROLLBACK TO browser_client; RELEASE browser_client")?;
                Err(error)
            }
        }
    }
    fn user_client_request_inner(
        &mut self,
        id: &str,
        credential: &str,
        body: Value,
        now: i64,
    ) -> Result<Value> {
        let admin = self.user_admin(id)?;
        let op = body["op"].as_str().ok_or(E::InvalidInput)?.to_string();
        if op == "selectionState" {
            if body.as_object().is_none_or(|o| {
                o.keys()
                    .any(|k| !["op", "slot", "owner"].contains(&k.as_str()))
            }) {
                return Err(E::InvalidInput);
            }
            let host = self.client_authenticate(credential)?;
            let a = self.delivery_open(
                &host,
                body["slot"].as_str().ok_or(E::InvalidInput)?,
                body["owner"].as_str().ok_or(E::InvalidInput)?,
            )?;
            return Ok(json!({"revision":a.revision}));
        }
        if op == "openSlot" {
            let host = self.client_authenticate(credential)?;
            if self.assignment_get(host.id(), None) == Err(E::NotFound) {
                let c = self.user_catalog(id)?;
                let d = c["defaultDashboard"]
                    .as_str()
                    .or_else(|| c["dashboards"][0]["id"].as_str())
                    .ok_or(E::NotFound)?;
                let a = Assignment {
                    dashboard_id: d.into(),
                    params: json!({}),
                    presentation: json!({}),
                };
                self.conn.execute(
                    "INSERT INTO dashboard_assignments VALUES(?,'',NULL,1,?)",
                    params![host.id(), serde_json::to_string(&a)?],
                )?;
            }
        }
        if ["openSlot", "delivery", "confirmDelivery", "select"].contains(&op.as_str()) {
            let host = self.client_authenticate(credential)?;
            let assignment = if op == "select" {
                serde_json::from_value::<Assignment>(body["assignment"].clone())?
            } else {
                self.assignment_get(
                    host.id(),
                    Some(body["slot"].as_str().ok_or(E::InvalidInput)?),
                )
                .or_else(|e| {
                    if e == E::NotFound && op == "openSlot" {
                        self.assignment_get(host.id(), None)
                    } else {
                        Err(e)
                    }
                })?
                .assignment
            };
            self.user_package(id, &assignment.dashboard_id)?;
            if op == "select" && !admin && assignment.params != json!({}) {
                return Err(E::Forbidden);
            }
        }
        // Status contains only this installation's metadata; never a global registry.
        let selection = if op == "select" {
            Some(serde_json::from_value::<Assignment>(
                body["assignment"].clone(),
            )?)
        } else {
            None
        };
        let mut result = self.client_request(credential, serde_json::from_value(body)?, now)?;
        if let Some(a) = selection {
            let host = self.client_authenticate(credential)?;
            let default = self.assignment_get(host.id(), None)?;
            self.assignment_set(host.id(), None, default.revision, &a)?;
        }
        if !admin && op == "delivery" {
            result["package"]["grants"]["writes"] = json!([]);
            result["package"]["grants"]["runs"] = json!([]);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
