//! Native notification enrollment and the backend-only sender connection.
use crate::{deployment::Deployment, *};
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use std::{io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};
use talia_engine::notification_outbox::Subscription;
#[derive(Clone)]
pub struct Worker {
    engine: Engine,
    deployment: Deployment,
    config: Rc<RefCell<Value>>,
    path: PathBuf,
    client: reqwest::Client,
    ready: Rc<Cell<bool>>,
    renewed: Rc<Cell<i64>>,
}
impl Worker {
    pub fn load(engine: Engine, deployment: Option<Deployment>) -> Result<Option<Self>> {
        let Some(path) = std::env::var_os("TALIA_STORE_NOTIFICATIONS_FILE") else {
            return Ok(None);
        };
        let path = PathBuf::from(path);
        let mut config: Value =
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let url = url::Url::parse(config["url"].as_str().ok_or("notification URL required")?)
            .map_err(|e| e.to_string())?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("notification server must be an HTTPS origin".into());
        }
        if config["applications"].as_array().is_none_or(|a| {
            a.is_empty()
                || a.iter().any(|p| {
                    !matches!(
                        p.as_str(),
                        Some("com.lelloman.talia" | "com.lelloman.talia.normal")
                    )
                })
        }) {
            return Err("notification applications required".into());
        }
        if config["credential"].is_null() {
            let mut bytes = [0u8; 32];
            SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| "randomness unavailable")?;
            config["credential"] =
                json!(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
            config["registration_id"] = config["credential"].clone();
            save(&path, &config)?;
        }
        Ok(Some(Self {
            engine,
            deployment: deployment.ok_or("notifications require OIDC")?,
            config: Rc::new(RefCell::new(config)),
            path,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| e.to_string())?,
            ready: Rc::new(Cell::new(false)),
            renewed: Rc::new(Cell::new(0)),
        }))
    }
    async fn call(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value> {
        let config = self.config.borrow().clone();
        let response = self
            .client
            .request(
                method,
                format!(
                    "{}/api/notifications/v1{path}",
                    config["url"].as_str().unwrap().trim_end_matches('/')
                ),
            )
            .bearer_auth(
                config["credential"]
                    .as_str()
                    .ok_or("sender credential missing")?,
            )
            .json(&body)
            .send()
            .await
            .map_err(|_| "notification server unavailable")?;
        if !response.status().is_success() {
            return Err(format!(
                "notification server status {}",
                response.status().as_u16()
            ));
        }
        response
            .json()
            .await
            .map_err(|_| "invalid notification response".into())
    }
    async fn initialize(&self) -> Result<()> {
        if self.ready.get() {
            return Ok(());
        }
        let c = self.config.borrow().clone();
        if !c["invitation"].is_null() {
            // If the registration response was lost, the persisted credential can
            // still install the manifest even after the invitation has expired.
            let _=self.call(reqwest::Method::POST,"/senders/register",json!({"invitation":c["invitation"],"registration_id":c["registration_id"],"credential":c["credential"]})).await;
        }
        let types:Vec<Value>=c["applications"].as_array().unwrap().iter().flat_map(|app|[
            json!({"application":app,"name":"report.completed","levels":["nominal","warning","error","unknown"],"default":{"strategy":"queue","ttl_seconds":null}}),
            json!({"application":app,"name":"incident.state","levels":["info","warning","critical","error"],"default":{"strategy":"latest","ttl_seconds":null}}),
        ]).collect();
        self.call(
            reqwest::Method::PUT,
            "/sender/manifest",
            json!({"types":types,"rules":[]}),
        )
        .await?;
        if !c["invitation"].is_null() {
            let mut next = self.config.borrow().clone();
            next.as_object_mut().unwrap().remove("invitation");
            save(&self.path, &next)?;
            *self.config.borrow_mut() = next;
        }
        self.ready.set(true);
        Ok(())
    }
    pub async fn enroll(&self, owner: &str, session: &str, args: &Value) -> Result<Value> {
        if !self
            .engine
            .store
            .borrow()
            .user_admin(owner)
            .map_err(|_| "forbidden")?
        {
            return Err("forbidden".into());
        }
        self.initialize().await?;
        let identity = self
            .deployment
            .identity_id(session)
            .await
            .map_err(|_| "unauthenticated")?;
        if identity.subject != owner {
            return Err("forbidden".into());
        }
        let (issuer, subject) = owner.rsplit_once('#').ok_or("invalid identity")?;
        let proof = args["proof"].as_str().ok_or("proof required")?;
        let result = self
            .call(
                reqwest::Method::POST,
                "/subscriptions",
                json!({"proof":proof,"issuer":issuer,"subject":subject}),
            )
            .await?;
        let id = result["subscription_id"]
            .as_str()
            .ok_or("missing subscription")?;
        // Package comes from Store's verified Binder enrollment, never from app claims.
        let app = result["application"]
            .as_str()
            .ok_or("missing application")?;
        if !self.config.borrow()["applications"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == app)
        {
            return Err("application not configured".into());
        }
        if !self
            .engine
            .store
            .borrow()
            .user_admin(owner)
            .map_err(|_| "forbidden")?
        {
            return Err("forbidden".into());
        }
        self.engine
            .store
            .borrow()
            .notification_subscribe(&Subscription {
                id: id.into(),
                owner: owner.into(),
                subject: subject.into(),
                session: session.into(),
                application: app.into(),
            })?;
        Ok(result)
    }
    pub async fn disable(&self, owner: &str, args: &Value) -> Result<Value> {
        let id = args["subscription_id"]
            .as_str()
            .ok_or("subscription required")?;
        let subscriptions = self.engine.store.borrow().notification_subscriptions()?;
        if !subscriptions.iter().any(|s| s.id == id && s.owner == owner) {
            return Err("forbidden".into());
        }
        self.call(
            reqwest::Method::DELETE,
            &format!("/subscriptions/{id}"),
            json!({}),
        )
        .await?;
        self.engine.store.borrow().notification_forget(id)?;
        Ok(json!({"ok":true}))
    }
    pub async fn tick(&self) -> Result<()> {
        self.initialize().await?;
        let now = self.engine.now();
        if now - self.renewed.get() >= 120000 {
            let subscriptions = self.engine.store.borrow().notification_subscriptions()?;
            for s in subscriptions {
                let identity = self.deployment.identity_id(&s.session).await;
                let permitted = self
                    .engine
                    .store
                    .borrow()
                    .user_admin(&s.owner)
                    .unwrap_or(false);
                match identity {
                    Ok(id) if permitted && id.subject == s.owner => {
                        match self
                            .call(
                                reqwest::Method::POST,
                                "/subscriptions/renew",
                                json!({"subscriptions":[s.id]}),
                            )
                            .await
                        {
                            Ok(_) => {}
                            Err(e)
                                if e == "notification server status 403"
                                    || e == "notification server status 404" =>
                            {
                                self.engine.store.borrow().notification_forget(&s.id)?
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(axum::http::StatusCode::SERVICE_UNAVAILABLE) => {
                        return Err("recipient authorization unavailable".into())
                    }
                    _ => {
                        self.call(
                            reqwest::Method::DELETE,
                            &format!("/subscriptions/{}", s.id),
                            json!({}),
                        )
                        .await?;
                        self.engine.store.borrow().notification_forget(&s.id)?;
                    }
                }
            }
            self.renewed.set(now);
        }
        let pending = self.engine.store.borrow().notification_pending()?;
        for (id, mut body) in pending {
            body["event_id"] = json!(digest::digest(&digest::SHA256, id.as_bytes())
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>());
            self.call(reqwest::Method::POST, "/messages", body).await?;
            self.engine
                .store
                .borrow()
                .notification_result(&id, "accepted")?;
        }
        self.engine.store.borrow().notification_prune()?;
        Ok(())
    }
}
fn save(path: &std::path::Path, value: &Value) -> Result<()> {
    let mut nonce = [0u8; 16];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| "randomness unavailable")?;
    let suffix = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let temp = path.with_extension(format!("pending-{suffix}"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|_| "cannot persist notification credential")?;
    file.write_all(value.to_string().as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| "cannot persist notification credential")?;
    std::fs::rename(&temp, path).map_err(|_| "cannot persist notification credential")?;
    std::fs::File::open(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new(".")),
    )
    .and_then(|dir| dir.sync_all())
    .map_err(|_| "cannot sync notification configuration directory")?;
    Ok(())
}
