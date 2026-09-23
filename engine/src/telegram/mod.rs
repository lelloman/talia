//! Server-owned Telegram delivery and observer conversations. No editing authority.
use crate::{
    runtime::Engine,
    store::{Result, Store},
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{cell::Cell, rc::Rc, time::Duration};
mod conversation;
pub mod observer;
pub mod transport;
use transport::{random, Bot};
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub const PROVIDER: &str = "talia-telegram";
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub version: u64,
    pub enabled: bool,
    pub bot_id: i64,
    pub username: String,
    pub token: String,
    pub offset: i64,
    pub observer: String,
    #[serde(default)]
    pub sources: Vec<String>,
    pub last_poll: Option<i64>,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Peer {
    pub chat: i64,
    pub name: String,
    pub kind: String,
    pub delivery: bool,
    pub investigate: bool,
}
impl Store {
    pub fn telegram_config(&self) -> Result<Config> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT body FROM telegram_config WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        v.map(|v| serde_json::from_str(&v).map_err(err))
            .unwrap_or_else(|| Ok(Config::default()))
    }
    fn telegram_put(&self, c: &Config) -> Result<()> {
        self.conn.execute("INSERT INTO telegram_config VALUES(1,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",[serde_json::to_string(c).map_err(err)?]).map_err(err)?;
        Ok(())
    }
    pub fn telegram_key_path(&self) -> Result<String> {
        if let Ok(p) = std::env::var("TALIA_TELEGRAM_KEY_FILE") {
            return Ok(p);
        }
        let p = self
            .conn
            .path()
            .filter(|p| !p.is_empty())
            .ok_or("persistent database required for Telegram")?;
        Ok(format!("{p}.telegram-key"))
    }
    fn telegram_peers(&self) -> Result<Vec<Peer>> {
        let mut q = self
            .conn
            .prepare("SELECT body FROM telegram_peers ORDER BY chat")
            .map_err(err)?;
        let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
        rows.map(|v| serde_json::from_str(&v.map_err(err)?).map_err(err))
            .collect()
    }
    fn telegram_authorized(&self, chat: i64, user: i64) -> Result<bool> {
        let allowed: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM telegram_users WHERE id=?)",
                [user],
                |r| r.get(0),
            )
            .map_err(err)?;
        Ok(allowed
            && self
                .telegram_peers()?
                .iter()
                .any(|p| p.chat == chat && p.investigate))
    }
    fn telegram_enqueue(
        &self,
        id: &str,
        chat: i64,
        kind: &str,
        reference: Option<&str>,
        text: &str,
    ) -> Result<()> {
        if text.len() > 131072 {
            return Err("Telegram message size limit".into());
        }
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM telegram_outbox", [], |r| r.get(0))
            .map_err(err)?;
        if count >= 10000 {
            return Err("Telegram outbox capacity reached".into());
        }
        // Parts are durable and individually tracked. Sending is never automatically retried.
        let chars: Vec<char> = text.chars().collect();
        for (index, part) in chars.chunks(1800).enumerate() {
            let body: String = part.iter().collect();
            self.conn.execute("INSERT OR IGNORE INTO telegram_outbox(id,chat,kind,reference,status,body) VALUES(?,?,?,?,'pending',?)",params![format!("{id}-{index:03}"),chat,kind,reference,body]).map_err(err)?;
        }
        Ok(())
    }
    pub fn telegram_delivery_status(&self, id: &str) -> Result<String> {
        let mut q = self
            .conn
            .prepare("SELECT status FROM telegram_outbox WHERE id LIKE ?")
            .map_err(err)?;
        let statuses = q
            .query_map([format!("{id}-%")], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(if statuses.is_empty() {
            "missing"
        } else if statuses.iter().any(|s| s == "unknown") {
            "unknown"
        } else if statuses.iter().any(|s| s == "failed") {
            "failed"
        } else if statuses.iter().all(|s| s == "sent") {
            "sent"
        } else {
            "pending"
        }
        .into())
    }
    pub fn telegram_report(
        &mut self,
        delivery: &str,
        chat: i64,
        run: &crate::reports::Run,
    ) -> Result<String> {
        if !self.telegram_config()?.enabled
            || !self
                .telegram_peers()?
                .iter()
                .any(|p| p.chat == chat && p.delivery)
        {
            return Err("Telegram destination disabled".into());
        }
        if self.telegram_delivery_status(delivery)? == "missing" {
            self.alert_atomic(|s| {
                s.telegram_enqueue(
                    delivery,
                    chat,
                    "report",
                    Some(&run.id),
                    run.text.as_deref().ok_or("report not composed")?,
                )
            })?;
        }
        self.conn
            .execute(
                "UPDATE telegram_outbox SET expires=? WHERE id LIKE ?",
                params![run.deadline, format!("{delivery}-%")],
            )
            .map_err(err)?;
        self.telegram_delivery_status(delivery)
    }
    pub fn telegram_recover(&self) -> Result<()> {
        self.conn
            .execute(
                "UPDATE telegram_outbox SET status='unknown' WHERE status='sending'",
                [],
            )
            .map_err(err)?;
        Ok(())
    }
}
#[derive(Clone)]
pub struct Worker {
    pub engine: Engine,
    active: Rc<Cell<bool>>,
    last: Rc<Cell<i64>>,
}
impl Worker {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            active: Default::default(),
            last: Rc::new(Cell::new(i64::MIN)),
        }
    }
    pub fn tick(&self) -> Result<()> {
        let now = self.engine.now();
        if self.active.get()
            || now.saturating_sub(self.last.get()) < 2000
            || !self.engine.store.borrow().telegram_config()?.enabled
        {
            return Ok(());
        }
        self.active.set(true);
        self.last.set(now);
        let this = self.clone();
        tokio::task::spawn_local(async move {
            let result = this.advance().await;
            {
                let s = this.engine.store.borrow();
                if let Ok(mut c) = s.telegram_config() {
                    c.error = result.err();
                    let _ = s.telegram_put(&c);
                }
            }
            this.active.set(false);
        });
        Ok(())
    }
    pub async fn advance(&self) -> Result<()> {
        let (c, bot) = Bot::load(&self.engine).await?;
        let updates=bot.call("getUpdates",json!({"offset":c.offset,"limit":20,"timeout":0,"allowed_updates":["message","channel_post"]})).await;
        if let Ok(updates) = &updates {
            let mut s = self.engine.store.borrow_mut();
            if s.telegram_config()?.version != c.version {
                return Ok(());
            }
            for update in updates.as_array().ok_or("invalid Telegram updates")? {
                s.telegram_ingest(update, self.engine.now())?;
            }
            let mut current = s.telegram_config()?;
            current.last_poll = Some(self.engine.now());
            s.telegram_put(&current)?;
        }
        self.conversation().await?;
        self.dispatch(&bot, c.version).await?;
        updates.map(|_| ())
    }
    async fn dispatch(&self, bot: &Bot, version: u64) -> Result<()> {
        let pending: Option<(String, i64, String, String, Option<String>)> = {
            let s = self.engine.store.borrow();
            s.conn.query_row("SELECT id,chat,body,kind,reference FROM telegram_outbox WHERE status='pending' ORDER BY rowid LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(err)?
        };
        let Some((id, chat, text, kind, reference)) = pending else {
            return Ok(());
        };
        {
            let s = self.engine.store.borrow();
            let c = s.telegram_config()?;
            if c.version != version || !c.enabled {
                return Ok(());
            }
            let expired: bool = s
                .conn
                .query_row(
                    "SELECT expires>0 AND expires<=? FROM telegram_outbox WHERE id=?",
                    params![self.engine.now(), id],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if expired {
                s.conn
                    .execute(
                        "UPDATE telegram_outbox SET status='failed' WHERE id=?",
                        [id],
                    )
                    .map_err(err)?;
                return Ok(());
            }
            let permitted = if kind == "chat" {
                reference
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .is_some_and(|u| s.telegram_authorized(chat, u).unwrap_or(false))
            } else {
                s.telegram_peers()?
                    .iter()
                    .any(|p| p.chat == chat && p.delivery)
            };
            if !permitted {
                s.conn
                    .execute(
                        "UPDATE telegram_outbox SET status='failed' WHERE id=?",
                        [id],
                    )
                    .map_err(err)?;
                return Ok(());
            }
            s.conn
                .execute(
                    "UPDATE telegram_outbox SET status='sending' WHERE id=?",
                    [&id],
                )
                .map_err(err)?;
        }
        let result = bot
            .call(
                "sendMessage",
                json!({"chat_id":chat,"text":text,"link_preview_options":{"is_disabled":true}}),
            )
            .await;
        let mid = result
            .ok()
            .and_then(|v| v["message_id"].as_i64())
            .filter(|v| *v > 0);
        self.engine
            .store
            .borrow()
            .conn
            .execute(
                "UPDATE telegram_outbox SET status=?,message_id=? WHERE id=?",
                params![if mid.is_some() { "sent" } else { "unknown" }, mid, id],
            )
            .map_err(err)?;
        Ok(())
    }
    pub async fn admin(&self, subject: &str, args: Value) -> Result<Value> {
        self.require_admin(subject)?;
        let op = args["op"].as_str().ok_or("operation required")?;
        if op == "telegramStatus" {
            let profiles = observer::profiles().await.unwrap_or_default();
            self.require_admin(subject)?;
            let s = self.engine.store.borrow();
            let c = s.telegram_config()?;
            let mut q = s
                .conn
                .prepare(
                    "SELECT code,expires,body FROM telegram_pairs WHERE expires>? ORDER BY expires",
                )
                .map_err(err)?;
            let pairs=q.query_map([self.engine.now()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<String>>(2)?))).map_err(err)?.map(|r|{let(code,expires,body)=r.map_err(err)?;Ok(json!({"code":code,"expires":expires,"candidate":body.and_then(|v|serde_json::from_str::<Value>(&v).ok())}))}).collect::<Result<Vec<_>>>()?;
            let mut q = s
                .conn
                .prepare("SELECT id,name FROM telegram_users ORDER BY id")
                .map_err(err)?;
            let users = q
                .query_map([], |r| {
                    Ok(json!({"id":r.get::<_,i64>(0)?,"name":r.get::<_,String>(1)?}))
                })
                .map_err(err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(err)?;
            let peers = s.telegram_peers()?;
            let mut q = s
                .conn
                .prepare("SELECT status,count(*) FROM telegram_outbox GROUP BY status")
                .map_err(err)?;
            let deliveries = q
                .query_map([], |r| {
                    Ok(json!({"status":r.get::<_,String>(0)?,"count":r.get::<_,i64>(1)?}))
                })
                .map_err(err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(err)?;
            let mut jobs=s.conn.prepare("SELECT id,status,json_extract(body,'$.error'),json_extract(body,'$.pending.session_id') FROM telegram_jobs ORDER BY id DESC LIMIT 20").map_err(err)?;
            let jobs=jobs.query_map([],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"status":r.get::<_,String>(1)?,"error":r.get::<_,Option<String>>(2)?,"session":r.get::<_,Option<String>>(3)?}))).map_err(err)?.collect::<std::result::Result<Vec<_>,_>>().map_err(err)?;
            return Ok(
                json!({"jobs":jobs,"version":c.version,"enabled":c.enabled,"bot":c.username,"observer":c.observer,"sources":c.sources,"profiles":profiles,"lastPoll":c.last_poll,"error":c.error,"peers":peers,"users":users,"pairs":pairs,"deliveries":deliveries}),
            );
        }
        if op == "telegramConnect" {
            let token = args["token"].as_str().ok_or("bot token required")?;
            let bot = Bot {
                token: token.into(),
                base: transport::base(),
            };
            let me = bot.call("getMe", json!({})).await?;
            if me["is_bot"] != true {
                return Err("not a Telegram bot".into());
            }
            if bot.call("getWebhookInfo", json!({})).await?["url"]
                .as_str()
                .is_none_or(|s| !s.is_empty())
            {
                return Err(
                    "This bot has a webhook; use a dedicated bot without another consumer".into(),
                );
            }
            let path = self.engine.store.borrow().telegram_key_path()?;
            let sealed = transport::seal(&transport::key(path, true).await?, token)?;
            self.require_admin(subject)?;
            let s = self.engine.store.borrow();
            let mut c = s.telegram_config()?;
            if args["expected"].as_u64() != Some(c.version) {
                return Err("Telegram configuration changed; refresh".into());
            }
            let bot_id = me["id"]
                .as_i64()
                .filter(|id| *id > 0)
                .ok_or("invalid bot identity")?;
            if c.bot_id != 0 && c.bot_id != bot_id {
                return Err("Cannot replace this installation's bot with another identity".into());
            }
            c.bot_id = bot_id;
            c.username = me["username"]
                .as_str()
                .ok_or("bot username missing")?
                .into();
            c.token = sealed;
            c.enabled = true;
            c.version += 1;
            c.error = None;
            s.telegram_put(&c)?;
            return Ok(json!({"connected":true}));
        }
        if op == "telegramSettings" {
            let name = args["observer"].as_str().ok_or("observer required")?;
            if !name.is_empty() {
                observer::provider(name).await?;
            }
            self.require_admin(subject)?;
            let s = self.engine.store.borrow();
            let mut c = s.telegram_config()?;
            if args["expected"].as_u64() != Some(c.version) {
                return Err("Telegram configuration changed; refresh".into());
            }
            let sources: Vec<String> = serde_json::from_value(args["sources"].clone())
                .map_err(|_| "source IDs required")?;
            if sources.len() > 32 {
                return Err("too many sources".into());
            }
            for source in &sources {
                s.monitoring_config()?.source(source)?;
            }
            if c.observer != name {
                s.conn.execute("UPDATE telegram_jobs SET status='cancelled' WHERE status IN ('queued','running')",[]).map_err(err)?;
                s.conn.execute("UPDATE telegram_outbox SET status='failed' WHERE kind='chat' AND status='pending'",[]).map_err(err)?;
            }
            c.observer = name.into();
            c.sources = sources;
            c.enabled = args["enabled"].as_bool().ok_or("enabled required")?;
            if c.enabled && c.token.is_empty() {
                return Err("Connect a bot first".into());
            }
            c.version += 1;
            s.telegram_put(&c)?;
            return Ok(json!({"saved":true}));
        }
        let mut s = self.engine.store.borrow_mut();
        match op {
            "telegramPair"=>{let code=random()?;s.conn.execute("DELETE FROM telegram_pairs WHERE expires<=?",[self.engine.now()]).map_err(err)?;let n:i64=s.conn.query_row("SELECT count(*) FROM telegram_pairs",[],|r|r.get(0)).map_err(err)?;if n>=10{return Err("pairing limit".into());}s.conn.execute("INSERT INTO telegram_pairs VALUES(?,?,NULL)",params![code,self.engine.now()+600000]).map_err(err)?;Ok(json!({"code":code}))},
            "telegramApprove"=>s.alert_atomic(|s|{
                let code=args["code"].as_str().ok_or("code required")?;
                let body:String=s.conn.query_row("SELECT body FROM telegram_pairs WHERE code=? AND expires>?",params![code,self.engine.now()],|r|r.get(0)).map_err(|_|"pairing expired or not claimed")?;
                let candidate:Value=serde_json::from_str(&body).map_err(err)?;
                let chat=candidate["chat"].as_i64().ok_or("chat missing")?;
                let delivery=args["delivery"].as_bool().ok_or("delivery required")?;
                let investigate=args["investigate"].as_bool().ok_or("investigate required")?;
                if investigate&&candidate["user"].as_i64().is_none(){return Err("Channels are delivery-only; pair a personal account separately".into());}
                let p=Peer{chat,name:candidate["name"].as_str().unwrap_or("").chars().take(128).collect(),kind:candidate["kind"].as_str().unwrap_or("").into(),delivery,investigate};
                s.telegram_save_peer(&p,self.engine.now())?;
                if investigate{let user=candidate["user"].as_i64().unwrap();s.conn.execute("INSERT INTO telegram_users VALUES(?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name",params![user,candidate["user_name"].as_str().unwrap_or("")]).map_err(err)?;}
                s.conn.execute("DELETE FROM telegram_pairs WHERE code=?",[code]).map_err(err)?;Ok(json!({"approved":true}))
            }),
            "telegramPeer"=>{let mut p:Peer=serde_json::from_value(args["peer"].clone()).map_err(|_|"invalid destination")?;let old=s.telegram_peers()?.into_iter().find(|v|v.chat==p.chat).ok_or("pair the destination first")?;p.kind=old.kind;p.name=old.name;if p.kind=="channel"&&p.investigate{return Err("channel investigations unavailable".into());}s.telegram_save_peer(&p,self.engine.now())?;Ok(json!({"saved":true}))},
            "telegramRevokeUser"=>{let user=args["id"].as_i64().ok_or("numeric user ID required")?;s.conn.execute("DELETE FROM telegram_users WHERE id=?",[user]).map_err(err)?;s.conn.execute("UPDATE telegram_jobs SET status='cancelled' WHERE user=? AND status IN ('queued','running')",[user]).map_err(err)?;s.conn.execute("UPDATE telegram_outbox SET status='failed' WHERE kind='chat' AND reference=? AND status='pending'",[user.to_string()]).map_err(err)?;Ok(json!({"revoked":true}))},
            "telegramTest"=>{let chat=args["chat"].as_i64().ok_or("chat required")?;if !s.telegram_peers()?.iter().any(|p|p.chat==chat&&p.delivery){return Err("delivery destination not enabled".into());}let id=random()?;s.telegram_enqueue(&id,chat,"test",None,"Talìa Telegram connection test.")?;Ok(json!({"queued":true}))},
            "telegramObserverKey"=>{let token=format!("to_{}",random()?);s.conn.execute("INSERT INTO telegram_observer VALUES(1,?) ON CONFLICT(id) DO UPDATE SET hash=excluded.hash",[transport::hash(&token)]).map_err(err)?;Ok(json!({"token":token}))},
            "telegramObserverRevoke"=>{s.conn.execute("DELETE FROM telegram_observer",[]).map_err(err)?;Ok(json!({"revoked":true}))},
            _=>Err("unknown Telegram operation".into())
        }
    }
    fn require_admin(&self, subject: &str) -> Result<()> {
        if !self
            .engine
            .store
            .borrow()
            .user_admin(subject)
            .map_err(|_| "forbidden")?
        {
            return Err("forbidden".into());
        }
        Ok(())
    }
}
impl Store {
    fn telegram_save_peer(&mut self, p: &Peer, now: i64) -> Result<()> {
        if p.chat == 0
            || p.name.len() > 256
            || !["private", "group", "supergroup", "channel"].contains(&p.kind.as_str())
        {
            return Err("invalid Telegram destination".into());
        }
        if self.telegram_peers()?.len() >= 100
            && !self.telegram_peers()?.iter().any(|v| v.chat == p.chat)
        {
            return Err("destination capacity".into());
        }
        if !p.investigate {
            self.conn.execute("UPDATE telegram_jobs SET status='cancelled' WHERE chat=? AND status IN ('queued','running')",[p.chat]).map_err(err)?;
            self.conn.execute("UPDATE telegram_outbox SET status='failed' WHERE chat=? AND kind='chat' AND status='pending'",[p.chat]).map_err(err)?;
        }
        let id = format!("telegram-{}", p.chat);
        let old = self.alert_destinations()?.into_iter().find(|d| d.id == id);
        let version = old.map_or(0, |d| d.version);
        self.alert_destination_save(
            &crate::alerts::delivery::Destination {
                id,
                version: version + 1,
                channel: "telegram".into(),
                provider: PROVIDER.into(),
                target: p.chat.to_string(),
                enabled: p.delivery,
            },
            version,
            "telegram-admin",
            now,
        )?;
        self.conn.execute("INSERT INTO telegram_peers VALUES(?,?) ON CONFLICT(chat) DO UPDATE SET body=excluded.body",params![p.chat,serde_json::to_string(p).map_err(err)?]).map_err(err)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests;
