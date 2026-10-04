//! Server-owned app chat (web and native). Administrators hold explicit sessions with
//! Talìa's read-only investigation tools; each request is answered through the shared
//! conversation core, so long sessions are selectively summarized like Telegram ones.
//!
//! Sessions replace Telegram's session classifier: a request always continues its
//! session. Client request IDs make create and send idempotent, so retries never
//! duplicate messages. Stop and delete only change state; in-flight inference is
//! discarded by the chat scope check that guards every step.
use crate::{
    ai,
    conversation::{self as core, ContextKey, Selection, Turn, ANSWER, SUMMARIZE},
    runtime::Engine,
    store::{Result, Store},
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

const REQUEST_TIMEOUT_MS: i64 = 10 * 60 * 1000;
const MAX_TEXT: usize = 8000;
const MAX_TITLE: usize = 120;
const MAX_SESSIONS: i64 = 200;
const MAX_ACTIVE_PER_USER: i64 = 4;
const MAX_ACTIVE_PER_SESSION: i64 = 2;
const MAX_REQUESTS: i64 = 20_000;
const CONCURRENT_SESSIONS: usize = 4;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Persisted per-request progress; resumable after a restart.
#[derive(Clone, Serialize, Deserialize)]
struct Job {
    id: i64,
    session: String,
    subject: String,
    text: String,
    report: Option<String>,
    created: i64,
    deadline: i64,
    phase: String,
    ai_run: Option<String>,
    #[serde(default)]
    context_revision: i64,
    #[serde(default)]
    batches: u8,
    #[serde(default)]
    offered: Vec<String>,
    #[serde(default)]
    maintenance_deadline: Option<i64>,
    #[serde(default)]
    maintenance_error: Option<String>,
}
impl Job {
    fn key(&self) -> ContextKey {
        ContextKey::chat(&self.session)
    }
    fn scope(&self) -> ai::Scope {
        ai::Scope::Chat {
            request: self.id,
            session: self.session.clone(),
            subject: self.subject.clone(),
        }
    }
    fn question(&self) -> String {
        match &self.report {
            Some(id) => format!("{}\n[Referenced report: {id}]", self.text),
            None => self.text.clone(),
        }
    }
}

fn session_id() -> Result<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0u8; 16];
    SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| "random generator unavailable")?;
    Ok(format!(
        "chat-{}",
        b.iter().map(|v| format!("{v:02x}")).collect::<String>()
    ))
}
fn client_id(args: &Value) -> Result<String> {
    let id = args["requestId"].as_str().ok_or("requestId required")?;
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err("invalid requestId".into());
    }
    Ok(id.into())
}
fn text(args: &Value) -> Result<String> {
    let text = args["text"].as_str().ok_or("text required")?.trim();
    if text.is_empty() || text.chars().count() > MAX_TEXT {
        return Err("message must contain 1 to 8000 characters".into());
    }
    Ok(text.into())
}
fn title(value: &str) -> String {
    let line = value.lines().next().unwrap_or("").trim();
    let mut title: String = line.chars().take(MAX_TITLE).collect();
    if title.is_empty() {
        title = "New chat".into();
    }
    title
}

impl Store {
    fn chat_job_put(&self, j: &Job, status: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE chat_requests SET body=?,status=? WHERE id=?",
                params![serde_json::to_string(j).map_err(err)?, status, j.id],
            )
            .map_err(err)?;
        Ok(())
    }
    fn chat_owned(&self, subject: &str, session: &str) -> Result<Value> {
        self.conn
            .query_row(
                "SELECT id,title,report,created,updated FROM chat_sessions WHERE id=? AND subject=? AND deleted=0",
                params![session, subject],
                |r| {
                    Ok(json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"report":r.get::<_,Option<String>>(2)?,
                        "created":r.get::<_,i64>(3)?,"updated":r.get::<_,i64>(4)?}))
                },
            )
            .optional()
            .map_err(err)?
            .ok_or_else(|| "not_found".into())
    }
    /// Admits one request, or returns the existing one for a repeated client ID.
    fn chat_admit(
        &self,
        subject: &str,
        session: &str,
        client: &str,
        text: String,
        report: Option<String>,
        now: i64,
    ) -> Result<i64> {
        if let Some(id) = self
            .conn
            .query_row(
                "SELECT id FROM chat_requests WHERE session=? AND client_id=?",
                params![session, client],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?
        {
            return Ok(id);
        }
        let (user, own, total): (i64, i64, i64) = self
            .conn
            .query_row(
                "SELECT (SELECT count(*) FROM chat_requests r JOIN chat_sessions c ON c.id=r.session WHERE c.subject=? AND r.status IN ('queued','running')),
                        (SELECT count(*) FROM chat_requests WHERE session=? AND status IN ('queued','running')),
                        (SELECT count(*) FROM chat_requests)",
                params![subject, session],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(err)?;
        if user >= MAX_ACTIVE_PER_USER || own >= MAX_ACTIVE_PER_SESSION || total >= MAX_REQUESTS {
            return Err("limit_exceeded".into());
        }
        self.conn
            .execute(
                "INSERT INTO chat_requests(session,client_id,status,body,created) VALUES(?,?,'queued','{}',?)",
                params![session, client, now],
            )
            .map_err(err)?;
        let id = self.conn.last_insert_rowid();
        let job = Job {
            id,
            session: session.into(),
            subject: subject.into(),
            text,
            report,
            created: now,
            deadline: now.saturating_add(REQUEST_TIMEOUT_MS),
            phase: "maintain".into(),
            ai_run: None,
            context_revision: 0,
            batches: 0,
            offered: vec![],
            maintenance_deadline: None,
            maintenance_error: None,
        };
        self.chat_job_put(&job, "queued")?;
        self.conn
            .execute(
                "UPDATE chat_sessions SET updated=? WHERE id=?",
                params![now, session],
            )
            .map_err(err)?;
        Ok(id)
    }
    fn chat_view(&self, subject: &str, session: &str, after: i64) -> Result<Value> {
        let meta = self.chat_owned(subject, session)?;
        let mut q = self
            .conn
            .prepare("SELECT id,client_id,status,body,answer,error,created,finished FROM chat_requests WHERE session=? AND id>? ORDER BY id LIMIT 200")
            .map_err(err)?;
        let rows = q
            .query_map(params![session, after], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                ))
            })
            .map_err(err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)?;
        let mut requests = vec![];
        for (id, client, status, body, answer, error, created, finished) in rows {
            let job: Job = serde_json::from_str(&body).map_err(err)?;
            let running = ["queued", "running"].contains(&status.as_str());
            requests.push(json!({"id":id,"requestId":client,"status":status,"text":job.text,"report":job.report,
                "answer":answer,"error":error,"created":created,"finished":finished,
                "steps":if running {self.chat_steps(&job)?} else {json!([])}}));
        }
        Ok(json!({"session":meta,"requests":requests}))
    }
    /// User-facing progress derived from the request's AI runs; never raw tool output.
    fn chat_steps(&self, j: &Job) -> Result<Value> {
        let mut steps = vec![];
        if j.batches > 0 || j.phase == "maintain" && j.ai_run.is_some() {
            steps.push(
                json!({"label":"Condensing earlier conversation","done":j.phase != "maintain"}),
            );
        }
        if let Some(run) = self.ai_run(&format!("chat-{}-answer", j.id))? {
            let done: BTreeSet<&str> = run
                .messages
                .iter()
                .filter(|m| m["role"] == "tool")
                .filter_map(|m| m["tool_call_id"].as_str())
                .collect();
            for m in &run.messages {
                for call in m["tool_calls"].as_array().into_iter().flatten() {
                    let f = &call["function"];
                    let args: Value = serde_json::from_str(f["arguments"].as_str().unwrap_or("{}"))
                        .unwrap_or(json!({}));
                    let id = args["id"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(80)
                        .collect::<String>();
                    let label = match f["name"].as_str().unwrap_or("") {
                        "monitoring_snapshot" => "Checked the monitoring overview".to_string(),
                        "monitoring_read" => match args["kind"].as_str() {
                            Some("history") => format!("Read {id} history"),
                            Some("report") => format!("Read report {id}"),
                            _ => format!("Read {id}"),
                        },
                        "monitoring_probe" => format!(
                            "Queried {}",
                            args["source"]
                                .as_str()
                                .unwrap_or("a source")
                                .chars()
                                .take(80)
                                .collect::<String>()
                        ),
                        _ => continue,
                    };
                    steps.push(json!({"label":label,"done":call["id"].as_str().is_some_and(|c|done.contains(c))}));
                }
            }
        }
        Ok(json!(steps))
    }
}

/// Trusted host entry point; the subject comes from the authenticated session.
pub fn request(store: &mut Store, subject: &str, op: &str, args: Value, now: i64) -> Result<Value> {
    if !store.user_admin(subject).map_err(|_| "forbidden")? {
        return Err("forbidden".into());
    }
    if !args.is_object() {
        return Err("invalid_input".into());
    }
    match op {
        "list" => {
            let mut q = store
                .conn
                .prepare("SELECT c.id,c.title,c.report,c.created,c.updated,
                    (SELECT count(*) FROM chat_requests r WHERE r.session=c.id AND r.status IN ('queued','running'))
                    FROM chat_sessions c WHERE c.subject=? AND c.deleted=0 ORDER BY c.updated DESC LIMIT 200")
                .map_err(err)?;
            let sessions = q
                .query_map([subject], |r| {
                    Ok(json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"report":r.get::<_,Option<String>>(2)?,
                        "created":r.get::<_,i64>(3)?,"updated":r.get::<_,i64>(4)?,"running":r.get::<_,i64>(5)?>0}))
                })
                .map_err(err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(err)?;
            Ok(json!({"sessions":sessions}))
        }
        "create" => {
            let client = client_id(&args)?;
            let text = text(&args)?;
            let report = match args["report"].as_str() {
                Some(id) => {
                    store.report_run(id).map_err(|_| "report run not found")?;
                    Some(id.to_string())
                }
                None => None,
            };
            store.alert_atomic(|s| {
                let existing: Option<String> = s
                    .conn
                    .query_row("SELECT id FROM chat_sessions WHERE subject=? AND client_id=?", params![subject, client], |r| r.get(0))
                    .optional()
                    .map_err(err)?;
                let session = match existing {
                    Some(id) => id,
                    None => {
                        let count: i64 = s
                            .conn
                            .query_row("SELECT count(*) FROM chat_sessions WHERE subject=? AND deleted=0", [subject], |r| r.get(0))
                            .map_err(err)?;
                        if count >= MAX_SESSIONS {
                            return Err("limit_exceeded".into());
                        }
                        let id = session_id()?;
                        let name = args["title"].as_str().map(title).unwrap_or_else(|| title(&text));
                        s.conn
                            .execute(
                                "INSERT INTO chat_sessions(id,subject,client_id,title,report,created,updated) VALUES(?,?,?,?,?,?,?)",
                                params![id, subject, client, name, report, now, now],
                            )
                            .map_err(err)?;
                        id
                    }
                };
                let request = s.chat_admit(subject, &session, &client, text, report.clone(), now)?;
                Ok(json!({"session":session,"request":request}))
            })
        }
        "get" => {
            let session = args["session"].as_str().ok_or("session required")?;
            store.chat_view(subject, session, args["after"].as_i64().unwrap_or(0))
        }
        "send" => {
            let session = args["session"].as_str().ok_or("session required")?;
            let client = client_id(&args)?;
            let text = text(&args)?;
            store.alert_atomic(|s| {
                s.chat_owned(subject, session)?;
                let request = s.chat_admit(subject, session, &client, text, None, now)?;
                Ok(json!({"session":session,"request":request}))
            })
        }
        "stop" => {
            let session = args["session"].as_str().ok_or("session required")?;
            store.chat_owned(subject, session)?;
            let stopped = store
                .conn
                .execute(
                    "UPDATE chat_requests SET status='stopped',finished=? WHERE session=? AND status IN ('queued','running')",
                    params![now, session],
                )
                .map_err(err)?;
            Ok(json!({"stopped":stopped}))
        }
        "rename" => {
            let session = args["session"].as_str().ok_or("session required")?;
            let name = args["title"].as_str().ok_or("title required")?;
            if name.trim().is_empty() {
                return Err("title required".into());
            }
            store.chat_owned(subject, session)?;
            store
                .conn
                .execute(
                    "UPDATE chat_sessions SET title=? WHERE id=?",
                    params![title(name), session],
                )
                .map_err(err)?;
            store.chat_owned(subject, session)
        }
        "delete" => {
            let session = args["session"].as_str().ok_or("session required")?;
            store.chat_owned(subject, session)?;
            store.alert_atomic(|s| {
                s.conn
                    .execute(
                        "UPDATE chat_requests SET status='stopped',finished=? WHERE session=? AND status IN ('queued','running')",
                        params![now, session],
                    )
                    .map_err(err)?;
                s.conn.execute("UPDATE chat_sessions SET deleted=1 WHERE id=?", [session]).map_err(err)?;
                Ok(json!({"deleted":true}))
            })
        }
        _ => Err("unknown chat operation".into()),
    }
}

/// Advances queued requests: sessions run concurrently (bounded), requests within a
/// session strictly in order.
#[derive(Clone)]
pub struct Worker {
    pub engine: Engine,
    active: Rc<RefCell<BTreeSet<String>>>,
}
impl Worker {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            active: Default::default(),
        }
    }
    pub fn tick(&self) -> Result<()> {
        if self.active.borrow().len() >= CONCURRENT_SESSIONS {
            return Ok(());
        }
        let sessions: Vec<String> = {
            let s = self.engine.store.borrow();
            let mut q = s
                .conn
                .prepare("SELECT session FROM chat_requests WHERE status IN ('queued','running') GROUP BY session ORDER BY min(id) LIMIT 50")
                .map_err(err)?;
            let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
            rows.collect::<std::result::Result<_, _>>().map_err(err)?
        };
        for session in sessions {
            if self.active.borrow().len() >= CONCURRENT_SESSIONS {
                break;
            }
            if !self.active.borrow_mut().insert(session.clone()) {
                continue;
            }
            let this = self.clone();
            tokio::task::spawn_local(async move {
                if let Err(e) = this.advance_session(&session).await {
                    eprintln!("Chat worker: {e}");
                }
                this.active.borrow_mut().remove(&session);
            });
        }
        Ok(())
    }
    async fn advance_session(&self, session: &str) -> Result<()> {
        let body: Option<String> = self
            .engine
            .store
            .borrow()
            .conn
            .query_row(
                "SELECT body FROM chat_requests WHERE session=? AND status IN ('queued','running') ORDER BY id LIMIT 1",
                [session],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        let Some(body) = body else { return Ok(()) };
        let mut job: Job = serde_json::from_str(&body).map_err(err)?;
        if ai::permit(&self.engine, &job.scope()).is_err() {
            // Deleted session or revoked admin: retire without an answer.
            self.engine.store.borrow().chat_job_put(&job, "cancelled")?;
            return Ok(());
        }
        if let Err(error) = self.advance(&mut job).await {
            if ai::permit(&self.engine, &job.scope()).is_err() {
                return Ok(()); // Stopped or deleted while running.
            }
            let timed_out = self.engine.now() >= job.deadline
                || error.contains("AI execution deadline exceeded")
                || error == "Investigation deadline exceeded";
            self.fail(&mut job, &error, timed_out)?;
        }
        Ok(())
    }
    fn guard(&self, j: &Job) -> Result<()> {
        ai::permit(&self.engine, &j.scope())?;
        if self.engine.now() >= j.deadline {
            return Err("Investigation deadline exceeded".into());
        }
        Ok(())
    }
    fn commit(&self, j: &mut Job, st: &core::State) -> Result<()> {
        self.guard(j)?;
        self.engine.store.borrow_mut().alert_atomic(|s| {
            core::put_state(s, &j.key(), st, j.context_revision)?;
            j.context_revision += 1;
            s.chat_job_put(j, "running")
        })
    }
    async fn advance(&self, j: &mut Job) -> Result<()> {
        loop {
            self.guard(j)?;
            let mut st = core::state(&self.engine.store.borrow(), &j.key())?;
            j.context_revision = st.revision;
            if st.cutoff.is_none() {
                // The first request starts the session's working context.
                st.cutoff = Some(j.id);
                self.commit(j, &st)?;
                continue;
            }
            let (entries, more) = core::active(&self.engine.store.borrow(), &j.key(), &st, j.id)?;
            if j.phase == "maintain" {
                let raw = core::unprocessed_chat(
                    &self.engine.store.borrow(),
                    &j.key(),
                    st.cutoff.unwrap_or(j.id),
                    j.id,
                )?;
                if !core::needs_maintenance(&entries, more, raw)
                    || entries.is_empty()
                    || j.batches >= 2
                {
                    j.phase = "answer".into();
                    self.commit(j, &st)?;
                    continue;
                }
                if j.offered.is_empty() {
                    j.offered = core::offer(&self.engine.store.borrow(), &j.key(), &entries)?;
                }
                let offered = j
                    .offered
                    .iter()
                    .map(|id| core::entry(&self.engine.store.borrow(), &j.key(), id))
                    .collect::<Result<Vec<_>>>()?;
                let id = format!("chat-{}-summarize-{}", j.id, j.batches);
                j.ai_run = Some(id.clone());
                let deadline = *j
                    .maintenance_deadline
                    .get_or_insert(self.engine.now().saturating_add(180_000).min(j.deadline));
                self.engine.store.borrow().chat_job_put(j, "running")?;
                let result = if offered.is_empty() {
                    Err("context source exceeds maintenance batch limit".into())
                } else {
                    ai::execute_with_options(
                        &self.engine,
                        &id,
                        j.scope(),
                        deadline.min(j.deadline),
                        SUMMARIZE,
                        json!({"question":j.text,"entries":offered}),
                        false,
                        ai::ExecutionOptions {
                            max_tokens: 8192,
                            max_turns: 1,
                            max_request_chars: None,
                        },
                    )
                    .await
                    .and_then(|v| {
                        serde_json::from_str::<Selection>(
                            v["summary"].as_str().ok_or("summary missing")?,
                        )
                        .map_err(err)
                    })
                    .and_then(|v| {
                        core::validate_selection(&offered, &v)?;
                        Ok(v)
                    })
                };
                self.guard(j)?;
                match result {
                    Ok(selection) => self.engine.store.borrow_mut().alert_atomic(|s| {
                        core::put_state(s, &j.key(), &st, j.context_revision)?;
                        core::apply_selection(s, &j.key(), &offered, &selection, j.id, j.batches)?;
                        j.context_revision += 1;
                        j.batches += 1;
                        j.offered.clear();
                        j.ai_run = None;
                        j.maintenance_deadline = None;
                        s.chat_job_put(j, "running")
                    })?,
                    Err(e) => {
                        // Never replace valid context with an incomplete model response.
                        j.maintenance_error = Some(e);
                        j.phase = "answer".into();
                        j.offered.clear();
                        j.ai_run = None;
                        self.commit(j, &st)?;
                    }
                }
                continue;
            }
            if j.phase == "answer" {
                let input = core::answer_input(
                    &self.engine.store.borrow(),
                    &j.text,
                    None,
                    j.report.as_deref(),
                    &entries,
                    more,
                    false,
                )?;
                let id = format!("chat-{}-answer", j.id);
                j.ai_run = Some(id.clone());
                self.engine.store.borrow().chat_job_put(j, "running")?;
                let result = ai::execute(
                    &self.engine,
                    &id,
                    j.scope(),
                    j.deadline,
                    ANSWER,
                    input,
                    true,
                )
                .await?;
                self.guard(j)?;
                let answer = result["summary"]
                    .as_str()
                    .ok_or("AI answer missing")?
                    .to_string();
                let now = self.engine.now();
                return self.engine.store.borrow_mut().alert_atomic(|s| {
                    core::put_state(s, &j.key(), &st, j.context_revision)?;
                    let turn = Turn {
                        request: j.id,
                        question: j.question(),
                        answer: &answer,
                        deactivate: false,
                        ai_run: j.ai_run.as_deref(),
                    };
                    core::record_turn(s, &j.key(), &turn, now)?;
                    j.context_revision += 1;
                    s.chat_job_put(j, "done")?;
                    s.conn
                        .execute(
                            "UPDATE chat_requests SET answer=?,finished=? WHERE id=?",
                            params![answer, now, j.id],
                        )
                        .map_err(err)?;
                    s.conn
                        .execute(
                            "UPDATE chat_sessions SET updated=? WHERE id=?",
                            params![now, j.session],
                        )
                        .map_err(err)?;
                    Ok(())
                });
            }
            return Err("Unknown chat phase".into());
        }
    }
    fn fail(&self, j: &mut Job, error: &str, timed_out: bool) -> Result<()> {
        let now = self.engine.now();
        let message = if timed_out {
            "The request timed out before an answer was ready. You can send it again.".to_string()
        } else {
            match core::failure_reason(error) {
                Some(reason) => format!("The investigation failed: {reason}"),
                None => {
                    "The investigation failed. Its run and error are retained in Talìa.".to_string()
                }
            }
        };
        let outcome = if timed_out {
            "[Talìa request outcome: timed out. No completed answer is available; this does not mean no diagnostic work was attempted.]"
        } else {
            "[Talìa request outcome: failed. No completed answer is available; this does not mean no diagnostic work was attempted.]"
        };
        eprintln!("Chat request {} failed: {error}", j.id);
        self.engine.store.borrow_mut().alert_atomic(|s| {
            let active: bool = s
                .conn
                .query_row(
                    "SELECT status IN ('queued','running') FROM chat_requests WHERE id=?",
                    [j.id],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if !active {
                return Ok(());
            }
            // Recording the outcome keeps later answers from assuming it succeeded.
            if core::state(s, &j.key())?.cutoff.is_some() {
                let turn = Turn {
                    request: j.id,
                    question: j.question(),
                    answer: outcome,
                    deactivate: false,
                    ai_run: None,
                };
                core::record_turn(s, &j.key(), &turn, now)?;
            }
            s.chat_job_put(j, "failed")?;
            s.conn
                .execute(
                    "UPDATE chat_requests SET error=?,finished=? WHERE id=?",
                    params![message, now, j.id],
                )
                .map_err(err)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
