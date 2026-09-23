//! Installation-owned OAuth account. Browser/MCP never receive bearer credentials.
use super::crypto;
use super::*;
#[derive(Clone, Default, Serialize, Deserialize)]
struct Account {
    version: u64,
    origin: String,
    model: String,
    issuer: String,
    client: String,
    token_endpoint: String,
    userinfo: String,
    #[serde(default)]
    revoke: String,
    phase: String,
    owner: String,
    identity: Value,
    secret: String,
    expires: i64,
    flow_expires: i64,
    next_poll: i64,
    interval: i64,
    code: String,
    verification: String,
    error: Option<String>,
}
impl Store {
    fn ai_account(&self) -> Result<Option<Account>> {
        let body: Option<String> = self
            .conn
            .query_row("SELECT body FROM ai_account WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(err)?;
        body.map(|v| serde_json::from_str(&v).map_err(err))
            .transpose()
    }
    fn ai_account_put(&self, a: &Account) -> Result<()> {
        self.conn.execute("INSERT INTO ai_account VALUES(1,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body", [serde_json::to_string(a).map_err(err)?]).map_err(err)?;
        Ok(())
    }
    fn ai_key_path(&self) -> Result<String> {
        if let Ok(p) = std::env::var("TALIA_AI_KEY_FILE") {
            return Ok(p);
        }
        Ok(format!(
            "{}.ai-key",
            self.conn
                .path()
                .filter(|p| !p.is_empty())
                .ok_or("persistent database required for AI account")?
        ))
    }
    pub fn ai_account_recover(&self) -> Result<()> {
        if let Some(mut a) = self.ai_account()? {
            if matches!(a.phase.as_str(), "refreshing" | "polling") {
                fail(
                    &mut a,
                    "Account request interrupted by restart; reconnect required",
                );
                self.ai_account_put(&a)?;
            }
        }
        Ok(())
    }
}
fn require_admin(e: &Engine, subject: &str) -> Result<()> {
    if !e
        .store
        .borrow()
        .user_admin(subject)
        .map_err(|_| "forbidden")?
    {
        return Err("forbidden".into());
    }
    Ok(())
}
fn current(e: &Engine, version: u64) -> Result<Account> {
    e.store
        .borrow()
        .ai_account()?
        .filter(|a| a.version == version)
        .ok_or("AI account changed; retry with current settings".into())
}
fn save(e: &Engine, a: &Account) -> Result<()> {
    current(e, a.version)?;
    e.store.borrow().ai_account_put(a)
}
fn fail(a: &mut Account, reason: &str) {
    a.phase = "reconnect".into();
    a.secret.clear();
    a.error = Some(reason.into());
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|_| "AI account HTTP client unavailable".into())
}
fn safe_url(s: &str) -> Result<url::Url> {
    let u = url::Url::parse(s).map_err(|_| "invalid account URL")?;
    let loopback = match u.host() {
        Some(url::Host::Ipv4(v)) => v.is_loopback(),
        Some(url::Host::Ipv6(v)) => v.is_loopback(),
        _ => false,
    };
    if s.len() > 2048
        || !(u.scheme() == "https" || u.scheme() == "http" && loopback)
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || u.query().is_some()
    {
        return Err("account URLs require HTTPS without credentials, query or fragment".into());
    }
    Ok(u)
}
fn endpoint(v: &Value, name: &str, issuer: &url::Url) -> Result<String> {
    let s = text(v, name, 2048)?;
    let u = safe_url(&s)?;
    if u.origin() != issuer.origin() {
        return Err("provider endpoint must belong to its issuer".into());
    }
    Ok(s)
}
fn text(v: &Value, k: &str, max: usize) -> Result<String> {
    v[k].as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= max)
        .map(str::to_owned)
        .ok_or(format!("invalid {k}"))
}
async fn response(request: reqwest::RequestBuilder) -> Result<(bool, Value)> {
    let mut r = request
        .send()
        .await
        .map_err(|_| "Account request failed; outcome may be unknown")?;
    let ok = r.status().is_success();
    let mut bytes = vec![];
    while let Some(chunk) = r
        .chunk()
        .await
        .map_err(|_| "Account response interrupted")?
    {
        if bytes.len() + chunk.len() > 32768 {
            return Err("Account response too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((
        ok,
        serde_json::from_slice(&bytes).map_err(|_| "Invalid account response")?,
    ))
}
async fn get(url: &str) -> Result<Value> {
    let (ok, v) = response(client()?.get(url)).await?;
    if !ok {
        return Err("Account discovery failed".into());
    }
    Ok(v)
}
async fn secrets(e: &Engine, a: &Account) -> Result<Value> {
    let path = e.store.borrow().ai_key_path()?;
    let key = crypto::key(path, false).await?;
    serde_json::from_str(&crypto::unseal(&key, &a.secret)?)
        .map_err(|_| "Invalid saved account credentials".into())
}
async fn seal(e: &Engine, v: &Value) -> Result<String> {
    let path = e.store.borrow().ai_key_path()?;
    crypto::seal(&crypto::key(path, true).await?, &v.to_string())
}
pub async fn configuration(e: &Engine) -> Result<Config> {
    if let Some(a) = e.store.borrow().ai_account()? {
        if !matches!(
            a.phase.as_str(),
            "connected" | "refreshing" | "validating_refresh"
        ) {
            return Err("Connect the dedicated account in Settings → simple-ai".into());
        }
        return Ok(Config {
            origin: a.origin,
            model: a.model,
            token_file: String::new(),
        });
    }
    super::configuration().await
}
pub async fn status(e: &Engine) -> Value {
    match configuration(e).await {
        Ok(c) => json!({"configured":true,"model":c.model}),
        Err(error) => json!({"configured":false,"error":error}),
    }
}
fn public(a: &Account, subject: &str) -> Value {
    let mut v = json!({"version":a.version,"origin":a.origin,"model":a.model,"phase":a.phase,"identity":a.identity,"error":a.error,"owned":a.owner==subject});
    if a.owner == subject && a.phase == "pending" {
        v["pending"] = json!({"code":a.code,"url":a.verification,"expires":a.flow_expires,"interval":a.interval});
    }
    v
}
/// Confirmed account configuration changes fence all in-flight inference/tool continuations.
pub fn check(e: &Engine, revision: Option<u64>) -> Result<()> {
    let a = e.store.borrow().ai_account()?;
    if match (a, revision) {
        (None, None) => true,
        (Some(a), Some(v)) => {
            a.version == v
                && matches!(
                    a.phase.as_str(),
                    "connected" | "refreshing" | "validating_refresh"
                )
        }
        _ => false,
    } {
        Ok(())
    } else {
        Err("AI account changed or disconnected".into())
    }
}
pub struct Session {
    pub config: Config,
    pub revision: Option<u64>,
    pub token: String,
}
pub async fn session(e: &Engine) -> Result<Session> {
    let _guard = e.ai_auth_lock.lock().await;
    let account = e.store.borrow().ai_account()?;
    let Some(mut a) = account else {
        let config = super::configuration().await?;
        let token = super::file(config.token_file.clone(), 4096)
            .await?
            .trim()
            .to_string();
        if !token.starts_with("sk-") || token.bytes().any(|c| c.is_ascii_whitespace()) {
            return Err("simple-ai API key is invalid".into());
        }
        check(e, None)?;
        return Ok(Session {
            config,
            token,
            revision: None,
        });
    };
    if a.phase == "validating_refresh" {
        validate(e, &mut a).await?;
    }
    if a.phase != "connected" {
        return Err("AI account requires connection or confirmation in Settings".into());
    }
    if a.expires <= e.now() + 30_000 {
        let v = secrets(e, &a).await?;
        a.phase = "refreshing".into();
        save(e, &a)?;
        let client_id = a.client.clone();
        let result = exchange(
            e,
            &mut a,
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &client_id),
                (
                    "refresh_token",
                    v["refresh"].as_str().ok_or("missing refresh token")?,
                ),
            ],
        )
        .await;
        if let Err(error) = result {
            fail(&mut a, &error);
            save(e, &a)?;
            return Err(error);
        }
        a.phase = "validating_refresh".into();
        save(e, &a)?;
        validate(e, &mut a).await?;
    }
    let token = text(&secrets(e, &a).await?, "access", 16384)?;
    check(e, Some(a.version))?;
    Ok(Session {
        config: Config {
            origin: a.origin,
            model: a.model,
            token_file: String::new(),
        },
        token,
        revision: Some(a.version),
    })
}
async fn exchange(e: &Engine, a: &mut Account, form: &[(&str, &str)]) -> Result<()> {
    let (ok, v) = response(client()?.post(&a.token_endpoint).form(form)).await?;
    current(e, a.version)?;
    if !ok {
        return Err(match v["error"].as_str() {
            Some("authorization_pending") => "authorization_pending",
            Some("slow_down") => "slow_down",
            Some("access_denied") => "Account authorization denied",
            Some("expired_token") => "Account authorization expired",
            Some("invalid_grant") => "Account session expired or revoked; reconnect required",
            _ => "Account token request rejected",
        }
        .into());
    }
    if v["token_type"]
        .as_str()
        .is_none_or(|s| !s.eq_ignore_ascii_case("bearer"))
    {
        return Err("Unsupported account token type".into());
    }
    let access = text(&v, "access_token", 16384)?;
    let refresh = text(&v, "refresh_token", 16384)?;
    let lifetime = v["expires_in"]
        .as_i64()
        .filter(|n| *n > 30 && *n <= 31_536_000)
        .ok_or("Invalid account token lifetime")?;
    a.secret = seal(e, &json!({"access":access,"refresh":refresh})).await?;
    a.expires = e.now() + lifetime * 1000;
    // Caller persists tokens before any further provider I/O.
    Ok(())
}
async fn validate(e: &Engine, a: &mut Account) -> Result<()> {
    let v = secrets(e, a).await?;
    current(e, a.version)?;
    let (ok, who) = response(
        client()?
            .get(&a.userinfo)
            .bearer_auth(text(&v, "access", 16384)?),
    )
    .await?;
    current(e, a.version)?;
    if !ok {
        fail(
            a,
            "Account identity could not be verified; reconnect required",
        );
        save(e, a)?;
        return Err(a.error.clone().unwrap());
    }
    let sub = text(&who, "sub", 256)?;
    if !a.identity.is_null() && a.identity["sub"] != sub {
        fail(a, "Account identity changed; reconnect required");
        save(e, a)?;
        return Err(a.error.clone().unwrap());
    }
    a.identity = json!({"sub":sub,"name":who["name"].as_str().or(who["preferred_username"].as_str()).map(|s|super::bounded_text(s,256)),"email":who["email"].as_str().map(|s|super::bounded_text(s,256))});
    a.phase = if a.phase == "validating_refresh" {
        "connected"
    } else {
        "confirm"
    }
    .into();
    a.error = None;
    save(e, a)
}
pub async fn admin(e: &Engine, subject: &str, args: Value) -> Result<Value> {
    require_admin(e, subject)?;
    let op = args["op"].as_str().ok_or("invalid operation")?;
    let allowed: &[&str] = match op {
        "aiAccountStatus" => &["op"],
        "aiAccountBegin" => &["op", "expected", "origin", "model"],
        "aiAccountPoll" | "aiAccountConfirm" | "aiAccountDisconnect" => &["op", "expected"],
        _ => return Err("unknown AI account operation".into()),
    };
    if args
        .as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err("invalid AI account fields".into());
    }
    let old = e.store.borrow().ai_account()?;
    let version = old.as_ref().map_or(0, |a| a.version);
    if op == "aiAccountStatus" {
        if let Some(a) = old {
            return Ok(public(&a, subject));
        }
        let configured = status(e).await;
        require_admin(e, subject)?;
        return Ok(json!({"version":0,"phase":"unconfigured","legacy":configured}));
    }
    if args["expected"].as_u64() != Some(version) {
        return Err("AI account changed; refresh settings".into());
    }
    if op == "aiAccountDisconnect" {
        // Durable tombstone: no implicit fallback to a mounted API key.
        let mut a = Account {
            version: version + 1,
            phase: "disconnected".into(),
            origin: old.as_ref().map(|a| a.origin.clone()).unwrap_or_default(),
            model: old.as_ref().map(|a| a.model.clone()).unwrap_or_default(),
            ..Default::default()
        };
        e.store.borrow().ai_account_put(&a)?;
        // Local disconnection is durable before any provider I/O. No waiting for refresh.
        if let Some(old) = old.filter(|a| !a.secret.is_empty() && !a.revoke.is_empty()) {
            let revoked = async {
                let secret = secrets(e, &old).await?;
                let Some(token) = secret["refresh"].as_str() else {
                    return Ok::<_, String>(());
                };
                let r = client()?
                    .post(&old.revoke)
                    .form(&[
                        ("client_id", old.client.as_str()),
                        ("token", token),
                        ("token_type_hint", "refresh_token"),
                    ])
                    .send()
                    .await
                    .map_err(|_| "revocation failed")?;
                if !r.status().is_success() {
                    return Err("revocation rejected".into());
                }
                Ok(())
            }
            .await;
            if revoked.is_err() && current(e, a.version).is_ok() {
                a.error=Some("Disconnected locally; provider revocation could not be confirmed. You can revoke this session in LelloAuth.".into());
                save(e, &a)?;
            }
        }
        require_admin(e, subject)?;
        return Ok(public(&current(e, a.version)?, subject));
    }
    if op == "aiAccountBegin" {
        let origin = text(&args, "origin", 2048)?;
        let u = safe_url(&origin)?;
        if u.path() != "/" {
            return Err("simple-ai URL must be an origin without a path".into());
        }
        let model = text(&args, "model", 256)?;
        let meta = get(&format!(
            "{}/.well-known/simple-ai",
            origin.trim_end_matches('/')
        ))
        .await?;
        let issuer = text(&meta, "issuer", 2048)?;
        let issuer_url = safe_url(&issuer)?;
        let client_id = text(&meta, "client_id", 256)?;
        let doc = get(&format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        ))
        .await?;
        if doc["issuer"] != issuer {
            return Err("Provider issuer mismatch".into());
        }
        if doc["device_account_link_endpoint"].is_null() {
            return Err("Update LelloAuth to support signing in with a separate account for this connection".into());
        }
        let link_endpoint = endpoint(&doc, "device_account_link_endpoint", &issuer_url)?;
        let device = endpoint(&doc, "device_authorization_endpoint", &issuer_url)?;
        let token_endpoint = endpoint(&doc, "token_endpoint", &issuer_url)?;
        let userinfo = endpoint(&doc, "userinfo_endpoint", &issuer_url)?;
        let revoke = if doc["revocation_endpoint"].is_null() {
            String::new()
        } else {
            endpoint(&doc, "revocation_endpoint", &issuer_url)?
        };
        let (ok, v) = response(client()?.post(device).form(&[
            ("client_id", client_id.as_str()),
            ("scope", "openid profile email"),
        ]))
        .await?;
        if !ok {
            return Err("Device authorization rejected: enable device flow for simple-ai’s advertised public LelloAuth client".into());
        }
        let device_code = text(&v, "device_code", 4096)?;
        let mut verification = safe_url(&link_endpoint)?;
        verification
            .query_pairs_mut()
            .append_pair("user_code", &text(&v, "user_code", 128)?);
        let verification = verification.to_string();
        let lifetime = v["expires_in"]
            .as_i64()
            .filter(|n| *n > 0 && *n <= 3600)
            .ok_or("Invalid device authorization lifetime")?;
        let interval = v["interval"].as_i64().unwrap_or(5).clamp(5, 60);
        let a = Account {
            version: version + 1,
            origin,
            model,
            issuer,
            client: client_id,
            token_endpoint,
            userinfo,
            revoke,
            phase: "pending".into(),
            owner: subject.into(),
            secret: seal(e, &json!({"device":device_code})).await?,
            flow_expires: e.now() + lifetime * 1000,
            interval,
            next_poll: e.now() + interval * 1000,
            code: text(&v, "user_code", 128)?,
            verification,
            ..Default::default()
        };
        require_admin(e, subject)?;
        if e.store
            .borrow()
            .ai_account()?
            .as_ref()
            .map_or(0, |a| a.version)
            != version
        {
            return Err("AI account changed during authorization".into());
        }
        e.store.borrow().ai_account_put(&a)?;
        return Ok(public(&a, subject));
    }
    let _guard = e.ai_auth_lock.lock().await;
    require_admin(e, subject)?;
    let mut a = current(e, version)?;
    if a.owner != subject {
        return Err("Only the administrator who started this connection can confirm it".into());
    }
    if op == "aiAccountConfirm" {
        if a.phase != "confirm" || a.expires <= e.now() || a.flow_expires <= e.now() {
            return Err("Account is not ready for confirmation; reconnect if expired".into());
        }
        a.phase = "connected".into();
        a.code.clear();
        a.verification.clear();
        save(e, &a)?;
    } else if op == "aiAccountPoll" {
        if matches!(a.phase.as_str(), "pending" | "validating" | "confirm")
            && a.flow_expires <= e.now()
        {
            fail(&mut a, "Account authorization expired; connect again");
            save(e, &a)?;
        }
        if a.phase == "pending" && e.now() >= a.next_poll {
            let v = secrets(e, &a).await?;
            require_admin(e, subject)?;
            a.phase = "polling".into();
            save(e, &a)?;
            let client_id = a.client.clone();
            let result = exchange(
                e,
                &mut a,
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", &client_id),
                    (
                        "device_code",
                        v["device"].as_str().ok_or("missing device code")?,
                    ),
                ],
            )
            .await;
            match result {
                Ok(()) => a.phase = "validating".into(),
                Err(error) if error == "authorization_pending" || error == "slow_down" => {
                    if error == "slow_down" {
                        a.interval = (a.interval + 5).min(300);
                    }
                    a.phase = "pending".into();
                    a.next_poll = e.now() + a.interval * 1000;
                }
                Err(error) => fail(&mut a, &error),
            }
            save(e, &a)?; // Persist rotation/consumption before userinfo or authorization checks.
        }
        require_admin(e, subject)?;
        if a.phase == "validating" {
            validate(e, &mut a).await?;
        }
    }
    require_admin(e, subject)?;
    Ok(public(&current(e, version)?, subject))
}
