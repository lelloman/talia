//! Durable reporting workflows. Only trusted report operators can author/run/read them.
use crate::{
    monitoring::Schedule,
    store::{Result, Store},
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub mod api;
pub mod email;
pub mod worker;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn id(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Err("invalid report identity".into())
    } else {
        Ok(())
    }
}
fn timeout() -> u64 {
    3_600_000
}
fn period() -> u64 {
    86_400_000
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub id: String,
    pub version: u64,
    pub enabled: bool,
    pub schedule: Option<Schedule>,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
    #[serde(default = "period")]
    pub period_ms: u64,
    pub steps: Vec<Step>,
    pub compose: String,
    #[serde(default)]
    pub destinations: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(flatten)]
    pub action: Action,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Read {
        variable: String,
    },
    Source {
        source: String,
        request: String,
    },
    Script {
        source: String,
    },
    Analysis {
        instructions: String,
        inputs: Vec<String>,
    },
    /// Historical step retained by a migration; never executable.
    Unavailable {
        reason: String,
        original: Value,
    },
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Output {
    pub status: String,
    pub value: Value,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub subject: String,
    pub summary: String,
    pub sections: Vec<Section>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub title: String,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Delivery {
    pub destination: String,
    pub version: u64,
    pub status: String,
    pub attempts: u32,
    pub next_at: i64,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub definition: Definition,
    pub actor: String,
    pub created: i64,
    pub deadline: i64,
    pub period_start: i64,
    pub status: String,
    pub send: bool,
    pub index: usize,
    pub outputs: BTreeMap<String, Output>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_execution: Option<Value>,
    pub content: Option<Content>,
    pub html: Option<String>,
    pub text: Option<String>,
    pub deliveries: Vec<Delivery>,
    pub error: Option<String>,
}
impl Run {
    pub fn context(&self) -> Value {
        json!({"report":self.definition.id,"run":self.id,"now":self.created,"period":{"start":self.period_start,"end":self.created},"steps":self.outputs})
    }
}
pub fn evaluate(source: &str, context: &Value) -> Result<Value> {
    let script = crate::script::Script::new()?;
    script.eval(&format!("globalThis.ctx=JSON.parse({});ctx.decode=TaliaValue.decode;globalThis.result=({source})(ctx);",serde_json::to_string(&context.to_string()).map_err(err)?))?;
    script.eval("if(result && typeof result.then==='function')throw Error('Report scripts must return synchronously');globalThis.encoded=JSON.stringify(result);if(typeof encoded!=='string'||encoded.length>32768)throw Error('Report output limit');")?;
    serde_json::from_str(&script.string("encoded")?)
        .map_err(|_| "invalid report script result".into())
}
impl Definition {
    pub fn validate(&self, store: &Store) -> Result<()> {
        id(&self.id)?;
        if self.version == 0
            || self.version > 9_007_199_254_740_000
            || self.steps.is_empty()
            || self.steps.len() > 16
            || !(1000..=86_400_000).contains(&self.timeout_ms)
            || self.period_ms > 31_536_000_000
            || self.destinations.len() > 16
        {
            return Err("report bounds".into());
        }
        if let Some(schedule) = &self.schedule {
            schedule.validate()?;
            if matches!(schedule,Schedule::Interval{every_ms} if *every_ms<60000) {
                return Err("report interval must be at least one minute".into());
            }
        }
        if self.enabled && self.schedule.is_some() && self.destinations.is_empty() {
            return Err("scheduled reports require email or Telegram destinations".into());
        }
        if self
            .destinations
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != self.destinations.len()
        {
            return Err("duplicate report destinations".into());
        }
        let script = crate::script::Script::new()?;
        script.eval(&format!(
            "if(typeof ({})!=='function')throw Error('composer must be a function');",
            self.compose
        ))?;
        let mut names = std::collections::BTreeSet::new();
        for step in &self.steps {
            id(&step.id)?;
            if names.contains(&step.id) {
                return Err("duplicate step".into());
            }
            match &step.action {
                Action::Read { variable } => {
                    store.instance(variable)?;
                }
                Action::Source { source, request } => {
                    store.monitoring_config()?.source(source)?;
                    script.eval(&format!("if(typeof ({request})!=='function')throw Error('request must be a function');"))?;
                }
                Action::Script { source } => script.eval(&format!(
                    "if(typeof ({source})!=='function')throw Error('script must be a function');"
                ))?,
                Action::Analysis {
                    instructions,
                    inputs,
                } => {
                    if instructions.trim().is_empty()
                        || instructions.len() > 16384
                        || inputs.len() > 16
                        || inputs.iter().any(|i| !names.contains(i))
                    {
                        return Err("Analysis inputs must name previous steps and instructions must be bounded".into());
                    }
                }
                Action::Unavailable { reason, .. } => return Err(reason.clone()),
            }
            names.insert(step.id.clone());
        }
        let destinations = store.alert_destinations()?;
        for target in &self.destinations {
            if !destinations.iter().any(|d| {
                &d.id == target && ["email", "telegram"].contains(&d.channel.as_str()) && d.enabled
            }) {
                return Err("enabled email or Telegram destination required".into());
            }
        }
        if serde_json::to_vec(self).map_err(err)?.len() > 131072 {
            return Err("report definition size limit".into());
        }
        Ok(())
    }
}
impl Store {
    pub fn report_definitions(&self) -> Result<Vec<Definition>> {
        let mut q = self
            .conn
            .prepare("SELECT body FROM report_definitions ORDER BY id")
            .map_err(err)?;
        let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
        rows.map(|r| serde_json::from_str(&r.map_err(err)?).map_err(err))
            .collect()
    }
    pub fn report_definition(&self, id: &str) -> Result<Definition> {
        let s: String = self
            .conn
            .query_row(
                "SELECT body FROM report_definitions WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .map_err(|_| "report not found")?;
        serde_json::from_str(&s).map_err(err)
    }
    pub fn report_save(&mut self, d: &Definition, expected: u64, now: i64) -> Result<()> {
        d.validate(self)?;
        let old = self
            .report_definitions()?
            .into_iter()
            .find(|v| v.id == d.id);
        if old.as_ref().map_or(0, |v| v.version) != expected || d.version != expected + 1 {
            return Err("report version conflict".into());
        }
        if old.is_none() && self.report_definitions()?.len() >= 100 {
            return Err("report definition capacity".into());
        }
        let due = if old
            .as_ref()
            .is_some_and(|v| v.enabled == d.enabled && v.schedule == d.schedule)
        {
            self.conn
                .query_row(
                    "SELECT next_due FROM report_definitions WHERE id=?",
                    [&d.id],
                    |r| r.get::<_, Option<i64>>(0),
                )
                .map_err(err)?
        } else if d.enabled {
            d.schedule.as_ref().map(|s| s.next_after(now)).transpose()?
        } else {
            None
        };
        self.conn.execute("INSERT INTO report_definitions VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body,next_due=excluded.next_due",params![d.id,serde_json::to_string(d).map_err(err)?,due]).map_err(err)?;
        Ok(())
    }
    pub fn report_run(&self, id: &str) -> Result<Run> {
        let body: String = self
            .conn
            .query_row("SELECT body FROM report_runs WHERE id=?", [id], |r| {
                r.get(0)
            })
            .map_err(|_| "report run not found")?;
        serde_json::from_str(&body).map_err(err)
    }
    pub fn report_put(&self, r: &Run) -> Result<()> {
        let body = serde_json::to_string(r).map_err(err)?;
        if body.len() > 1_048_576 {
            return Err("report run size limit".into());
        }
        self.conn.execute("INSERT INTO report_runs VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET status=excluded.status,body=excluded.body",params![r.id,r.definition.id,r.status,r.created,body]).map_err(err)?;
        Ok(())
    }
    pub fn report_start(
        &mut self,
        definition: &str,
        actor: &str,
        send: bool,
        now: i64,
    ) -> Result<Run> {
        let d = self.report_definition(definition)?;
        if d.steps
            .iter()
            .any(|s| matches!(s.action, Action::Unavailable { .. }))
        {
            return Err(
                "Report contains a retired step; update its definition before running".into(),
            );
        }
        if send && d.destinations.is_empty() {
            return Err("email or Telegram destination required for sending".into());
        }
        let active:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM report_runs WHERE report=? AND status IN ('queued','running'))",[definition],|r|r.get(0)).map_err(err)?;
        if active {
            return Err("report already running".into());
        }
        let count: u64 = self
            .conn
            .query_row("SELECT count(*) FROM report_runs", [], |r| r.get(0))
            .map_err(err)?;
        if count >= 10000 {
            return Err("report run capacity; prune old completed runs".into());
        }
        let mut bytes = [0u8; 16];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
            .map_err(|_| "randomness unavailable")?;
        let r = Run {
            id: format!(
                "report-{}",
                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
            ),
            deadline: now
                .checked_add(d.timeout_ms as i64)
                .ok_or("deadline overflow")?,
            period_start: now
                .checked_sub(d.period_ms as i64)
                .ok_or("period overflow")?,
            definition: d,
            actor: actor.into(),
            created: now,
            status: "queued".into(),
            send,
            index: 0,
            outputs: BTreeMap::new(),
            retired_execution: None,
            content: None,
            html: None,
            text: None,
            deliveries: vec![],
            error: None,
        };
        self.report_put(&r)?;
        Ok(r)
    }
    pub fn report_scheduled(&mut self, now: i64) -> Result<()> {
        let due: Vec<String> = {
            let mut q = self
                .conn
                .prepare("SELECT id FROM report_definitions WHERE next_due<=?")
                .map_err(err)?;
            let rows = q.query_map([now], |r| r.get(0)).map_err(err)?;
            rows.collect::<std::result::Result<_, _>>().map_err(err)?
        };
        for id in due {
            self.alert_atomic(|s|{let d=s.report_definition(&id)?;let active:bool=s.conn.query_row("SELECT EXISTS(SELECT 1 FROM report_runs WHERE report=? AND status IN ('queued','running'))",[&id],|r|r.get(0)).map_err(err)?;if d.enabled&&!active{s.report_start(&id,"scheduler",true,now)?;}let next=d.schedule.as_ref().map(|schedule|schedule.next_after(now)).transpose()?;s.conn.execute("UPDATE report_definitions SET next_due=? WHERE id=?",params![next,id]).map_err(err)?;Ok(())})?;
        }
        Ok(())
    }
    pub fn report_active(&self) -> Result<Vec<String>> {
        let mut q=self.conn.prepare("SELECT id FROM report_runs WHERE status IN ('queued','running','delivering') ORDER BY created LIMIT 100").map_err(err)?;
        let rows = q.query_map([], |r| r.get(0)).map_err(err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(err)
    }
    pub fn report_recover(&self) -> Result<()> {
        // Recovery must cover every delivery, beyond the worker's per-tick batch.
        let ids = {
            let mut q = self
                .conn
                .prepare("SELECT id FROM report_runs WHERE status='delivering'")
                .map_err(err)?;
            let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(err)?
        };
        for id in ids {
            let mut r = self.report_run(&id)?;
            for d in &mut r.deliveries {
                if d.status == "sending" {
                    d.status = "unknown".into();
                    d.error = Some("service restarted during delivery; no automatic resend".into());
                }
            }
            self.report_put(&r)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
