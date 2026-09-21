//! LelloAuth OIDC browser boundary. No shared dashboard keys or provider tokens in JavaScript.
use crate::oidc::OidcClient;
use axum::{
    body::Body,
    extract::{Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ring::{
    aead, digest,
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Clone)]
pub struct Identity {
    pub subject: String,
    pub name: String,
}
struct Login {
    browser: String,
    verifier: String,
    nonce: String,
    expires: i64,
}
struct Inner {
    db: Connection,
    logins: HashMap<String, Login>,
    checked: HashMap<String, i64>,
}
#[derive(Clone)]
pub struct Deployment {
    root: PathBuf,
    origin: String,
    issuer: String,
    client: Arc<OidcClient>,
    inner: Arc<Mutex<Inner>>,
    key: Arc<aead::LessSafeKey>,
    budget: Arc<tokio::sync::Semaphore>,
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn random() -> String {
    let mut b = [0u8; 32];
    SystemRandom::new().fill(&mut b).expect("OS randomness");
    URL_SAFE_NO_PAD.encode(b)
}
fn hash(s: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, s.as_bytes()).as_ref())
}
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|s| {
            s.trim()
                .strip_prefix(&format!("{name}="))
                .map(str::to_owned)
        })
        .filter(|s| s.len() == 43)
}
fn set_cookie(name: &str, value: &str, age: i64) -> String {
    format!("{name}={value}; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age={age}")
}
impl Deployment {
    pub async fn load() -> Result<Option<Self>, String> {
        if std::env::var_os("TALIA_ACCESS_TOKEN_FILE").is_some() {
            return Err("deployment-key authentication has been removed; configure OIDC".into());
        }
        let Some(issuer) = std::env::var_os("TALIA_OIDC_ISSUER") else {
            if std::env::var_os("TALIA_LISTEN").is_some()
                || std::env::var_os("TALIA_WEB_ROOT").is_some()
            {
                return Err("network deployment requires LelloAuth OIDC".into());
            }
            return Ok(None);
        };
        let issuer = issuer.into_string().map_err(|_| "invalid issuer")?;
        let origin = std::env::var("TALIA_PUBLIC_ORIGIN").map_err(|_| "public origin required")?;
        let u = url::Url::parse(&origin).map_err(|_| "invalid origin")?;
        if u.scheme() != "https" || u.origin().ascii_serialization() != origin {
            return Err("public origin must be HTTPS without a path".into());
        }
        let client_id = std::env::var("TALIA_OIDC_CLIENT_ID").map_err(|_| "client ID required")?;
        let secret = std::fs::read_to_string(
            std::env::var("TALIA_OIDC_SECRET_FILE").map_err(|_| "OIDC secret file required")?,
        )
        .map_err(|_| "cannot read OIDC secret")?
        .trim()
        .to_owned();
        let client = OidcClient::confidential(&issuer, &client_id, secret.clone())
            .await
            .map_err(|e| { #[cfg(test)] eprintln!("OIDC fixture initialization: {e}"); let _=e; "cannot initialize OIDC provider" })?;
        let root = PathBuf::from(std::env::var("TALIA_WEB_ROOT").map_err(|_| "web root required")?)
            .canonicalize()
            .map_err(|_| "web root unavailable")?;
        let db = Connection::open(
            std::env::var("TALIA_AUTH_DB").map_err(|_| "auth database path required")?,
        )
        .map_err(|_| "cannot open auth database")?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS config(id INTEGER PRIMARY KEY, fingerprint TEXT NOT NULL); CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, subject TEXT NOT NULL, name TEXT NOT NULL, token BLOB NOT NULL, expires INTEGER NOT NULL);").map_err(|_|"auth schema unavailable")?;
        let fingerprint = hash(&format!("{issuer}\0{client_id}\0{secret}\0{origin}"));
        let previous: Option<String> = db
            .query_row("SELECT fingerprint FROM config WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|_| "auth config unavailable")?;
        if previous.as_deref() != Some(&fingerprint) {
            db.execute("DELETE FROM sessions", [])
                .map_err(|_| "cannot invalidate sessions")?;
            db.execute("INSERT OR REPLACE INTO config VALUES(1,?)", [fingerprint])
                .map_err(|_| "cannot save auth configuration")?;
        }
        let material = digest::digest(
            &digest::SHA256,
            format!("talia-oidc-session-encryption-v1\0{secret}").as_bytes(),
        );
        let key = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, material.as_ref())
                .map_err(|_| "session key failure")?,
        );
        Ok(Some(Self {
            root,
            origin,
            issuer,
            client: Arc::new(client),
            inner: Arc::new(Mutex::new(Inner {
                db,
                logins: HashMap::new(),
                checked: HashMap::new(),
            })),
            key: Arc::new(key),
            budget: Arc::new(tokio::sync::Semaphore::new(16)),
        }))
    }
    fn seal(&self, token: &str) -> Result<Vec<u8>, StatusCode> {
        let mut nonce = [0u8; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut b = token.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut b,
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok([nonce.to_vec(), b].concat())
    }
    fn open(&self, mut bytes: Vec<u8>) -> Result<String, StatusCode> {
        if bytes.len() < 28 {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let nonce: [u8; 12] = bytes[..12].try_into().unwrap();
        let p = self
            .key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut bytes[12..],
            )
            .map_err(|_| StatusCode::UNAUTHORIZED)?;
        String::from_utf8(p.to_vec()).map_err(|_| StatusCode::UNAUTHORIZED)
    }
    async fn identity(&self, headers: &HeaderMap) -> Result<Identity, StatusCode> {
        let id = hash(&cookie(headers, "__Host-talia-session").ok_or(StatusCode::UNAUTHORIZED)?);
        let (subject, name, token, checked) = {
            let mut inner = self.inner.lock().unwrap();
            inner.checked.retain(|_, t| now() - *t < 30);
            let row: Option<(String, String, Vec<u8>)> = inner
                .db
                .query_row(
                    "SELECT subject,name,token FROM sessions WHERE id=? AND expires>?",
                    params![id, now()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            let (s, n, t) = row.ok_or(StatusCode::UNAUTHORIZED)?;
            (s, n, t, inner.checked.contains_key(&id))
        };
        if !checked {
            let _permit = self
                .budget
                .try_acquire()
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if !self
                .client
                .active(&self.open(token)?, &subject)
                .await
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            {
                self.inner
                    .lock()
                    .unwrap()
                    .db
                    .execute("DELETE FROM sessions WHERE id=?", [&id])
                    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
                return Err(StatusCode::UNAUTHORIZED);
            }
            // A concurrent logout must not be undone by introspection completing late.
            let mut inner = self.inner.lock().unwrap();
            let exists: bool = inner
                .db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=? AND expires>?)",
                    params![id, now()],
                    |r| r.get(0),
                )
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if !exists {
                return Err(StatusCode::UNAUTHORIZED);
            }
            inner.checked.insert(id, now());
        }
        Ok(Identity {
            subject: format!("{}#{}", self.issuer, subject),
            name,
        })
    }
    pub fn routes(&self) -> Router {
        Router::new()
            .route("/auth/login", get(login))
            .route("/auth/callback", get(callback))
            .route("/auth/session", get(session))
            .route("/auth/logout", post(logout))
            .fallback(get(asset))
            .with_state(self.clone())
    }
    pub async fn gate(State(d): State<Self>, mut r: Request, next: Next) -> Response {
        let path = r.uri().path();
        let browser_alert = path == "/alerts" && !r.headers().contains_key(header::AUTHORIZATION);
        let protected = matches!(
            path,
            "/engine" | "/clients" | "/account" | "/auth/session" | "/auth/logout"
        ) || browser_alert;
        if r.method() != axum::http::Method::GET
            && (r
                .headers()
                .get(header::ORIGIN)
                .is_some_and(|o| o.as_bytes() != d.origin.as_bytes())
                || protected && r.headers().get(header::ORIGIN).is_none())
        {
            return StatusCode::FORBIDDEN.into_response();
        }
        if protected {
            match d.identity(r.headers()).await {
                Ok(identity) => {
                    // A browser installation credential is scoped to its authenticated user.
                    if path == "/clients" {
                        if let Some(value) = r
                            .headers()
                            .get(header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                        {
                            let scoped = hex_digest(&format!("{}\0{value}", identity.subject));
                            r.headers_mut().insert(
                                header::AUTHORIZATION,
                                format!("Bearer {scoped}").parse().unwrap(),
                            );
                        }
                    }
                    r.extensions_mut().insert(identity);
                }
                Err(code) => return code.into_response(),
            }
        }
        let mut response = next.run(r).await;
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        response
            .headers_mut()
            .insert("referrer-policy", "no-referrer".parse().unwrap());
        response
            .headers_mut()
            .insert("x-content-type-options", "nosniff".parse().unwrap());
        response
    }
}
fn hex_digest(s: &str) -> String {
    digest::digest(&digest::SHA256, s.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
async fn login(State(d): State<Deployment>, headers: HeaderMap) -> Response {
    let state = random();
    let browser = random();
    let verifier = random();
    let nonce = random();
    let challenge =
        URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, verifier.as_bytes()).as_ref());
    let mut inner = d.inner.lock().unwrap();
    inner.logins.retain(|_, l| l.expires > now());
    if let Some(old) = cookie(&headers, "__Host-talia-login") {
        inner.logins.retain(|_, l| l.browser != hash(&old));
    }
    if inner.logins.len() >= 256 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    inner.logins.insert(
        hash(&state),
        Login {
            browser: hash(&browser),
            verifier,
            nonce: nonce.clone(),
            expires: now() + 600,
        },
    );
    drop(inner);
    (
        [(
            header::SET_COOKIE,
            set_cookie("__Host-talia-login", &browser, 600),
        )],
        Redirect::to(&d.client.authorization_url(
            &format!("{}/auth/callback", d.origin),
            &state,
            &nonce,
            &challenge,
        )),
    )
        .into_response()
}
#[derive(Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<String>,
}
async fn callback(
    State(d): State<Deployment>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> Response {
    let outcome = finish(&d, &headers, q).await;
    let mut response = match outcome {
        Ok((token, age)) => (
            [(
                header::SET_COOKIE,
                set_cookie("__Host-talia-session", &token, age),
            )],
            Redirect::to("/"),
        )
            .into_response(),
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            "Sign-in failed. Return to Talìa and try again.",
        )
            .into_response(),
    };
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie("__Host-talia-login", "", 0).parse().unwrap(),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie("__Host-talia", "", 0).parse().unwrap(),
    );
    response
}
async fn finish(
    d: &Deployment,
    headers: &HeaderMap,
    q: Callback,
) -> Result<(String, i64), StatusCode> {
    let state = q
        .state
        .filter(|s| s.len() == 43)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let browser = cookie(headers, "__Host-talia-login").ok_or(StatusCode::UNAUTHORIZED)?;
    let flow = {
        let mut inner = d.inner.lock().unwrap();
        let login = inner
            .logins
            .get(&hash(&state))
            .ok_or(StatusCode::UNAUTHORIZED)?;
        if login.browser != hash(&browser) || login.expires <= now() {
            return Err(StatusCode::UNAUTHORIZED);
        }
        inner.logins.remove(&hash(&state)).unwrap()
    };
    let _permit = d
        .budget
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let (claims, access) = d
        .client
        .exchange_session(
            &q.code.ok_or(StatusCode::UNAUTHORIZED)?,
            &format!("{}/auth/callback", d.origin),
            &flow.verifier,
            &flow.nonce,
        )
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    if access.is_empty()
        || !d
            .client
            .active(&access, &claims.sub)
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let expires = (claims.exp as i64).min(now() + 28800);
    if expires <= now() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let token = random();
    let encrypted = d.seal(&access)?;
    let mut inner = d.inner.lock().unwrap();
    let tx = inner
        .db
        .transaction()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    tx.execute("DELETE FROM sessions WHERE expires<=?", [now()])
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if let Some(old) = cookie(headers, "__Host-talia-session") {
        tx.execute("DELETE FROM sessions WHERE id=?", [hash(&old)])
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    }
    tx.execute("DELETE FROM sessions WHERE id IN (SELECT id FROM sessions WHERE subject=? ORDER BY expires DESC LIMIT -1 OFFSET 15)",[&claims.sub]).map_err(|_|StatusCode::SERVICE_UNAVAILABLE)?;
    let count: i64 = tx
        .query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if count >= 1024 {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    tx.execute(
        "INSERT INTO sessions VALUES(?,?,?,?,?)",
        params![
            hash(&token),
            claims.sub,
            claims
                .name
                .or(claims.preferred_username)
                .unwrap_or_else(|| "LelloAuth user".into()),
            encrypted,
            expires
        ],
    )
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    tx.commit().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok((token, expires - now()))
}
async fn session(Extension(id): Extension<Identity>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"subject":id.subject,"name":id.name}))
}
async fn logout(State(d): State<Deployment>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie(&headers, "__Host-talia-session") {
        let id = hash(&token);
        let mut inner = d.inner.lock().unwrap();
        if inner
            .db
            .execute("DELETE FROM sessions WHERE id=?", [&id])
            .is_err()
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        inner.checked.remove(&id);
    }
    (
        [(
            header::SET_COOKIE,
            set_cookie("__Host-talia-session", "", 0),
        )],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}
async fn asset(State(d): State<Deployment>, r: Request) -> Response {
    let path = if r.uri().path() == "/" {
        "/dashboard/web/index.html"
    } else {
        r.uri().path()
    };
    if path == "/dashboard/web/index.html" {
        match d.identity(r.headers()).await {
            Ok(_) => (),
            Err(StatusCode::UNAUTHORIZED) => {
                return (
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    include_str!("../../deploy/login.html"),
                )
                    .into_response()
            }
            Err(code) => return code.into_response(),
        }
    }
    if path.contains('%')
        || path.contains('\\')
        || path.split('/').any(|s| s == ".." || s.starts_with('.'))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(file) = d.root.join(path.trim_start_matches('/')).canonicalize() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !file.starts_with(&d.root) || !file.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mime = match file.extension().and_then(|x| x.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("wasm") => "application/wasm",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    match tokio::fs::read(file).await {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, mime)
            .body(Body::from(bytes))
            .unwrap(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oidc::test_support::{keys, valid_claims, CLIENT_ID};
    use serde_json::json;
    use tower::ServiceExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    async fn fixture() -> (MockServer, Deployment) {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/.well-known/openid-configuration")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"issuer":server.uri(),"authorization_endpoint":format!("{}/authorize",server.uri()),"token_endpoint":format!("{}/token",server.uri()),"jwks_uri":format!("{}/jwks",server.uri()),"introspection_endpoint":format!("{}/introspect",server.uri()),"response_types_supported":["code"],"id_token_signing_alg_values_supported":["RS256"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none","client_secret_basic"]}))).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"keys":[keys().0.jwk()]})),
            )
            .mount(&server)
            .await;
        let client = OidcClient::confidential(&server.uri(), CLIENT_ID, "a".repeat(64))
            .await
            .unwrap();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY,subject TEXT,name TEXT,token BLOB,expires INTEGER)").unwrap();
        let d = Deployment {
            root: PathBuf::from("/nonexistent"),
            origin: "https://talia.test".into(),
            issuer: server.uri(),
            client: Arc::new(client),
            inner: Arc::new(Mutex::new(Inner {
                db,
                logins: HashMap::new(),
                checked: HashMap::new(),
            })),
            key: Arc::new(aead::LessSafeKey::new(
                aead::UnboundKey::new(&aead::AES_256_GCM, &[7; 32]).unwrap(),
            )),
            budget: Arc::new(tokio::sync::Semaphore::new(16)),
        };
        (server, d)
    }
    async fn flow(d: &Deployment) -> (String, String, String, String) {
        let r = login(State(d.clone()), HeaderMap::new()).await;
        let u = url::Url::parse(r.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let q: HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        let cookie = r.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        (
            q["state"].clone(),
            q["nonce"].clone(),
            q["code_challenge"].clone(),
            cookie,
        )
    }
    fn headers(cookie: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, cookie.parse().unwrap());
        h
    }
    async fn token(server: &MockServer, nonce: &str, challenge: &str) {
        let mut claims = valid_claims(&server.uri());
        claims.nonce = Some(nonce.into());
        let jwt = keys().0.sign(&claims);
        let challenge = challenge.to_string();
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(move |r: &wiremock::Request| {
                let form: HashMap<_, _> =
                    url::form_urlencoded::parse(&r.body).into_owned().collect();
                assert_eq!(form["redirect_uri"], "https://talia.test/auth/callback");
                assert_eq!(form["grant_type"], "authorization_code");
                assert_eq!(
                    URL_SAFE_NO_PAD.encode(
                        digest::digest(&digest::SHA256, form["code_verifier"].as_bytes()).as_ref()
                    ),
                    challenge
                );
                assert!(r
                    .headers
                    .get("authorization")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("Basic "));
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id_token":jwt,"access_token":"private-provider-token"}))
            })
            .mount(server)
            .await;
        Mock::given(method("POST")).and(path("/introspect")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"active":true,"sub":"provider-subject-123","client_id":CLIENT_ID,"iss":server.uri()}))).mount(server).await;
    }
    #[tokio::test]
    async fn pkce_browser_binding_replay_session_logout_and_removed_key() {
        let (server, d) = fixture().await;
        let (state, nonce, challenge, browser) = flow(&d).await;
        token(&server, &nonce, &challenge).await;
        assert!(finish(
            &d,
            &headers(&format!("__Host-talia-login={}", random())),
            Callback {
                state: Some(state.clone()),
                code: Some("code".into())
            }
        )
        .await
        .is_err());
        let (session_token, _) = finish(
            &d,
            &headers(&browser),
            Callback {
                state: Some(state.clone()),
                code: Some("code".into()),
            },
        )
        .await
        .unwrap();
        assert!(finish(
            &d,
            &headers(&browser),
            Callback {
                state: Some(state),
                code: Some("code".into())
            }
        )
        .await
        .is_err());
        let session_cookie = format!("__Host-talia-session={session_token}");
        assert!(d
            .identity(&headers(&session_cookie))
            .await
            .unwrap()
            .subject
            .ends_with("#provider-subject-123"));
        let raw: Vec<u8> = d
            .inner
            .lock()
            .unwrap()
            .db
            .query_row("SELECT token FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert!(!raw.windows(22).any(|b| b == b"private-provider-token"));
        let mut old = headers(&format!("__Host-talia={}", "a".repeat(64)));
        old.insert("x-talia-access", "a".repeat(64).parse().unwrap());
        assert!(d.identity(&old).await.is_err());
        let app = Router::new()
            .route("/engine", post(|| async { "ok" }))
            .merge(d.routes())
            .layer(axum::middleware::from_fn_with_state(
                d.clone(),
                Deployment::gate,
            ));
        let cross = Request::builder()
            .method("POST")
            .uri("/engine")
            .header(header::COOKIE, &session_cookie)
            .header(header::ORIGIN, "https://evil.test")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(cross).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let missing = Request::builder()
            .method("POST")
            .uri("/engine")
            .header(header::COOKIE, &session_cookie)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(missing).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            logout(State(d.clone()), headers(&session_cookie))
                .await
                .status(),
            StatusCode::NO_CONTENT
        );
        assert!(d.identity(&headers(&session_cookie)).await.is_err());
    }
    #[tokio::test]
    async fn revoked_access_and_provider_outage_fail_closed() {
        let (server, d) = fixture().await;
        let (state, nonce, challenge, browser) = flow(&d).await;
        token(&server, &nonce, &challenge).await;
        let (s, _) = finish(
            &d,
            &headers(&browser),
            Callback {
                state: Some(state),
                code: Some("code".into()),
            },
        )
        .await
        .unwrap();
        let h = headers(&format!("__Host-talia-session={s}"));
        server.reset().await;
        Mock::given(path("/introspect"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(matches!(
            d.identity(&h).await,
            Err(StatusCode::SERVICE_UNAVAILABLE)
        ));
        server.reset().await;
        Mock::given(path("/introspect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"active":false})))
            .mount(&server)
            .await;
        assert!(matches!(
            d.identity(&h).await,
            Err(StatusCode::UNAUTHORIZED)
        ));
    }
    #[tokio::test]
    async fn wrong_nonce_and_expired_transaction_are_rejected() {
        let (server, d) = fixture().await;
        let (state, _nonce, challenge, browser) = flow(&d).await;
        token(&server, "wrong-nonce", &challenge).await;
        assert!(finish(
            &d,
            &headers(&browser),
            Callback {
                state: Some(state),
                code: Some("code".into())
            }
        )
        .await
        .is_err());
        let (state, _, _, browser) = flow(&d).await;
        d.inner
            .lock()
            .unwrap()
            .logins
            .get_mut(&hash(&state))
            .unwrap()
            .expires = now() - 1;
        assert!(finish(
            &d,
            &headers(&browser),
            Callback {
                state: Some(state),
                code: Some("code".into())
            }
        )
        .await
        .is_err());
    }
}
