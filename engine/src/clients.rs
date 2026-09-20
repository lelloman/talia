//! Host-owned registration. Neither guest state nor caller-supplied display names confer authority.
use crate::{
    authority::{ErrorCode as Error, Result},
    definitions::identifier,
    store::Store,
};
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub const LEASE_MS: i64 = 15_000;
const MAX_COUNTER: u64 = 9_007_199_254_740_990;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub dashboard_id: String,
    pub package_revision: String,
    pub lifecycle: String,
    pub foreground: bool,
    pub dirty: bool,
    pub edit_revision: u64,
    pub update_available: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub client_id: String,
    pub slot_id: String,
    pub live_instance_id: String,
    pub epoch: u64,
    pub sequence: u64,
    pub connected: bool,
    pub last_seen: i64,
    pub report: Report,
}
#[derive(Clone)]
pub struct Host {
    id: String,
}
impl Host {
    pub fn id(&self) -> &str {
        &self.id
    }
}
fn id(s: &str) -> Result<()> {
    identifier(s).map_err(|_| Error::InvalidInput)
}
fn secret(s: &str) -> Result<String> {
    if s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Unauthenticated);
    }
    Ok(digest::digest(&digest::SHA256, s.as_bytes())
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect())
}
fn random() -> Result<String> {
    let mut b = [0u8; 24];
    SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| Error::InternalError)?;
    Ok(b.iter().map(|v| format!("{v:02x}")).collect())
}
fn name_valid(s: &str) -> Result<()> {
    if s.trim().is_empty() || s.len() > 128 || s.chars().any(char::is_control) {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}
fn report_valid(r: &Report) -> Result<()> {
    id(&r.dashboard_id)?;
    id(&r.package_revision)?;
    if !["active", "paused", "failed"].contains(&r.lifecycle.as_str())
        || r.edit_revision > MAX_COUNTER
        || r.lifecycle == "active" && !r.foreground
    {
        return Err(Error::InvalidInput);
    }
    Ok(())
}
impl Store {
    /// A random installation credential makes registration idempotent even after response loss.
    pub fn client_register(
        &mut self,
        credential: &str,
        name: &str,
        platform: &str,
    ) -> Result<Value> {
        let digest = secret(credential)?;
        name_valid(name)?;
        if !["web", "android"].contains(&platform) {
            return Err(Error::InvalidInput);
        }
        let old: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT id,name,platform FROM dashboard_clients WHERE credential=?",
                [&digest],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((id, name, existing)) = old {
            if existing != platform {
                return Err(Error::Conflict);
            }
            return Ok(json!({"clientId":id,"name":name,"platform":platform}));
        }
        let n: u64 = self
            .conn
            .query_row("SELECT count(*) FROM dashboard_clients", [], |r| r.get(0))?;
        if n >= 256 {
            return Err(Error::LimitExceeded);
        }
        let id = random()?;
        self.conn.execute(
            "INSERT INTO dashboard_clients VALUES(?,?,?,?)",
            params![id, digest, name, platform],
        )?;
        Ok(json!({"clientId":id,"name":name,"platform":platform}))
    }
    pub fn client_authenticate(&self, credential: &str) -> Result<Host> {
        let digest = secret(credential)?;
        let id = self
            .conn
            .query_row(
                "SELECT id FROM dashboard_clients WHERE credential=?",
                [digest],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Unauthenticated)?;
        Ok(Host { id })
    }
    pub fn client_rename(&mut self, host: &Host, name: &str) -> Result<()> {
        name_valid(name)?;
        self.conn.execute(
            "UPDATE dashboard_clients SET name=? WHERE id=?",
            params![name, host.id],
        )?;
        Ok(())
    }
    fn client_slot(&self, host: &Host, slot: &str, owner: &str) -> Result<Slot> {
        id(slot)?;
        let owner = secret(owner)?;
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT owner,body FROM dashboard_slots WHERE client=? AND id=?",
                params![host.id, slot],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (stored, body) = row.ok_or(Error::NotFound)?;
        if stored != owner {
            return Err(Error::Forbidden);
        }
        Ok(serde_json::from_str(&body)?)
    }
    fn client_store_slot(&self, s: &Slot) -> Result<()> {
        self.conn.execute(
            "UPDATE dashboard_slots SET body=? WHERE client=? AND id=?",
            params![serde_json::to_string(s)?, s.client_id, s.slot_id],
        )?;
        Ok(())
    }
    /// Connection epochs are host-owned and monotonically increase across reload/reconnect.
    pub fn client_connect(
        &mut self,
        host: &Host,
        slot: &str,
        owner: &str,
        live: &str,
        previous: Option<&str>,
        epoch: u64,
        report: Report,
        now: i64,
    ) -> Result<Slot> {
        id(slot)?;
        id(live)?;
        if let Some(p) = previous {
            id(p)?;
        }
        let owner_digest = secret(owner)?;
        report_valid(&report)?;
        if epoch == 0 || epoch > MAX_COUNTER {
            return Err(Error::InvalidInput);
        }
        let existing = self.client_slot(host, slot, owner);
        let mut s = match existing {
            Ok(mut old) => {
                if epoch < old.epoch {
                    return Err(Error::StaleInstance);
                }
                if live != old.live_instance_id {
                    if epoch <= old.epoch || previous != Some(old.live_instance_id.as_str()) {
                        return Err(Error::StaleInstance);
                    }
                } else if report.edit_revision < old.report.edit_revision
                    || old.report.dirty && !report.dirty
                    || report.dashboard_id != old.report.dashboard_id
                    || report.package_revision != old.report.package_revision
                {
                    return Err(Error::Conflict);
                } else if epoch == old.epoch {
                    // Exact connection retry must not overwrite newer heartbeats or resurrect a disconnected epoch.
                    if !old.connected {
                        return Err(Error::StaleInstance);
                    }
                    return Ok(Self::client_observed(old, now));
                }
                old.live_instance_id = live.into();
                old.epoch = epoch;
                old.sequence = 0;
                old.connected = true;
                old.last_seen = now;
                old.report = report;
                old
            }
            Err(Error::NotFound) => {
                let n: u64 = self.conn.query_row(
                    "SELECT count(*) FROM dashboard_slots WHERE client=?",
                    [&host.id],
                    |r| r.get(0),
                )?;
                if n >= 64 {
                    return Err(Error::LimitExceeded);
                }
                Slot {
                    client_id: host.id.clone(),
                    slot_id: slot.into(),
                    live_instance_id: live.into(),
                    epoch,
                    sequence: 0,
                    connected: true,
                    last_seen: now,
                    report,
                }
            }
            Err(e) => return Err(e),
        };
        let known: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT client,slot FROM dashboard_instances WHERE id=?",
                [live],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if known.is_some() {
            let old = self.client_slot(host, slot, owner)?;
            if known != Some((host.id.clone(), slot.into())) || old.live_instance_id != live {
                return Err(Error::StaleInstance);
            }
        }
        let tx = self.conn.savepoint()?;
        tx.execute(
            "INSERT OR IGNORE INTO dashboard_instances VALUES(?,?,?)",
            params![live, host.id, slot],
        )?;
        s.connected = true;
        tx.execute("INSERT INTO dashboard_slots VALUES(?,?,?,?) ON CONFLICT(client,id) DO UPDATE SET body=excluded.body",params![host.id,slot,owner_digest,serde_json::to_string(&s)?])?;
        tx.commit()?;
        Ok(s)
    }
    pub fn client_report(
        &mut self,
        host: &Host,
        slot: &str,
        owner: &str,
        live: &str,
        epoch: u64,
        sequence: u64,
        report: Option<Report>,
        now: i64,
    ) -> Result<Slot> {
        let mut s = self.client_slot(host, slot, owner)?;
        if s.live_instance_id != live || s.epoch != epoch {
            return Err(Error::StaleInstance);
        }
        if !s.connected || now.saturating_sub(s.last_seen) >= LEASE_MS {
            return Err(Error::TargetUnavailable);
        }
        if sequence <= s.sequence || sequence > MAX_COUNTER {
            return Err(Error::Conflict);
        }
        if let Some(r) = report {
            report_valid(&r)?;
            if r.edit_revision < s.report.edit_revision
                || s.report.dirty && !r.dirty
                || r.dashboard_id != s.report.dashboard_id
                || r.package_revision != s.report.package_revision
            {
                return Err(Error::Conflict);
            }
            s.report = r;
        } else {
            s.connected = false;
        }
        s.sequence = sequence;
        s.last_seen = now;
        self.client_store_slot(&s)?;
        Ok(s)
    }
    fn client_observed(mut s: Slot, now: i64) -> Slot {
        s.connected = s.connected && now.saturating_sub(s.last_seen) < LEASE_MS;
        s
    }
    pub fn clients_recover(&mut self) -> Result<()> {
        let all = self.clients_list(0)?;
        for client in all {
            for s in client["slots"].as_array().unwrap() {
                let mut s: Slot = serde_json::from_value(s.clone())?;
                s.connected = false;
                self.client_store_slot(&s)?;
            }
        }
        Ok(())
    }
    /// Trusted discovery; agent adapters must additionally filter by live-list target scope.
    pub fn clients_list(&self, now: i64) -> Result<Vec<Value>> {
        let mut q = self
            .conn
            .prepare("SELECT id,name,platform FROM dashboard_clients ORDER BY id")?;
        let rows = q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = vec![];
        for row in rows {
            let (id, name, platform) = row?;
            let mut q = self
                .conn
                .prepare("SELECT body FROM dashboard_slots WHERE client=? ORDER BY id")?;
            let rows = q.query_map([&id], |r| r.get::<_, String>(0))?;
            let mut slots = vec![];
            for row in rows {
                slots.push(Self::client_observed(serde_json::from_str(&row?)?, now));
            }
            out.push(json!({"clientId":id,"name":name,"platform":platform,"slots":slots}));
        }
        Ok(out)
    }
    pub fn client_own_status(&self, host: &Host, now: i64) -> Result<Value> {
        self.clients_list(now)?
            .into_iter()
            .find(|c| c["clientId"] == host.id)
            .ok_or(Error::NotFound)
    }
    /// Gate used immediately before dispatch, never a deferred command queue.
    pub fn client_live_target(
        &self,
        client: &str,
        slot: &str,
        live: &str,
        execute: bool,
        now: i64,
    ) -> Result<Slot> {
        id(client)?;
        id(slot)?;
        id(live)?;
        let body: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM dashboard_slots WHERE client=? AND id=?",
                params![client, slot],
                |r| r.get(0),
            )
            .optional()?;
        let s = Self::client_observed(serde_json::from_str(&body.ok_or(Error::NotFound)?)?, now);
        if s.live_instance_id != live {
            return Err(Error::StaleInstance);
        }
        if !s.connected
            || !s.report.foreground
            || s.report.lifecycle == "paused"
            || execute && s.report.lifecycle == "failed"
        {
            return Err(Error::TargetUnavailable);
        }
        Ok(s)
    }
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    Register {
        name: String,
        platform: String,
    },
    Rename {
        name: String,
    },
    Status {},
    Connect {
        slot: String,
        owner: String,
        live: String,
        previous: Option<String>,
        epoch: u64,
        report: Report,
    },
    Report {
        slot: String,
        owner: String,
        live: String,
        epoch: u64,
        sequence: u64,
        report: Report,
    },
    Disconnect {
        slot: String,
        owner: String,
        live: String,
        epoch: u64,
        sequence: u64,
    },
}
impl Store {
    pub fn client_request(
        &mut self,
        credential: &str,
        request: Request,
        now: i64,
    ) -> Result<Value> {
        if let Request::Register { name, platform } = request {
            return self.client_register(credential, &name, &platform);
        }
        let host = self.client_authenticate(credential)?;
        match request {
            Request::Rename { name } => {
                self.client_rename(&host, &name)?;
                self.client_own_status(&host, now)
            }
            Request::Status {} => self.client_own_status(&host, now),
            Request::Connect {
                slot,
                owner,
                live,
                previous,
                epoch,
                report,
            } => Ok(serde_json::to_value(self.client_connect(
                &host,
                &slot,
                &owner,
                &live,
                previous.as_deref(),
                epoch,
                report,
                now,
            )?)?),
            Request::Report {
                slot,
                owner,
                live,
                epoch,
                sequence,
                report,
            } => Ok(serde_json::to_value(self.client_report(
                &host,
                &slot,
                &owner,
                &live,
                epoch,
                sequence,
                Some(report),
                now,
            )?)?),
            Request::Disconnect {
                slot,
                owner,
                live,
                epoch,
                sequence,
            } => Ok(serde_json::to_value(self.client_report(
                &host, &slot, &owner, &live, epoch, sequence, None, now,
            )?)?),
            _ => Err(Error::InvalidInput),
        }
    }
}
#[cfg(test)]
mod tests;
