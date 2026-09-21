//! Optional network deployment boundary. Development remains loopback-only.
use axum::{body::Body, extract::{Request, State}, http::{header, StatusCode}, middleware::Next, response::{IntoResponse, Response}, routing::{get, post}, Json, Router};
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
#[derive(Clone)]
pub struct Deployment { token: Arc<String>, root: PathBuf, origin: String }
impl Deployment {
    pub fn load() -> Result<Option<Self>, String> {
        let Some(path) = std::env::var_os("TALIA_ACCESS_TOKEN_FILE") else {
            if std::env::var_os("TALIA_LISTEN").is_some() || std::env::var_os("TALIA_WEB_ROOT").is_some() { return Err("network deployment requires TALIA_ACCESS_TOKEN_FILE".into()); }
            return Ok(None);
        };
        let token = std::fs::read_to_string(path).map_err(|_| "cannot read access token")?.trim().to_owned();
        if token.len()!=64 || !token.bytes().all(|b|b.is_ascii_hexdigit()) { return Err("access token must be 32 random bytes encoded as 64 hex characters".into()); }
        let root=PathBuf::from(std::env::var("TALIA_WEB_ROOT").map_err(|_| "TALIA_WEB_ROOT required")?).canonicalize().map_err(|_| "web root missing")?;
        let origin=std::env::var("TALIA_PUBLIC_ORIGIN").map_err(|_| "TALIA_PUBLIC_ORIGIN required")?;
        let u=url::Url::parse(&origin).map_err(|_| "invalid public origin")?;
        if u.scheme()!="https" || u.host_str().is_none() || u.origin().ascii_serialization()!=origin { return Err("public origin must be an HTTPS origin without path".into()); }
        Ok(Some(Self {token:Arc::new(token),root,origin}))
    }
    fn matches(&self, token:&str)->bool {
        let a=ring::digest::digest(&ring::digest::SHA256,token.as_bytes());
        let b=ring::digest::digest(&ring::digest::SHA256,self.token.as_bytes());
        a.as_ref().iter().zip(b.as_ref()).fold(0u8,|n,(x,y)|n|(x^y))==0
    }
    fn authorized(&self, r:&Request)->bool {
        r.headers().get("x-talia-access").and_then(|v|v.to_str().ok()).is_some_and(|s|self.matches(s)) ||
        r.headers().get(header::COOKIE).and_then(|v|v.to_str().ok()).is_some_and(|s|s.split(';').any(|p|p.trim().strip_prefix("__Host-talia=").is_some_and(|v|self.matches(v))))
    }
    pub fn routes(&self)->Router {Router::new().route("/session",post(login)).fallback(get(asset)).with_state(self.clone())}
    pub async fn gate(State(d):State<Self>,r:Request,next:Next)->Response {
        if r.method()!=axum::http::Method::GET && r.headers().get(header::ORIGIN).is_some_and(|o|o.as_bytes()!=d.origin.as_bytes()) {return StatusCode::FORBIDDEN.into_response();}
        // Agent and alert endpoints already enforce their independent principal grants.
        let path=r.uri().path();
        if matches!(path,"/engine"|"/clients") && !d.authorized(&r) {return StatusCode::UNAUTHORIZED.into_response();}
        let mut response=next.run(r).await;
        response.headers_mut().insert(header::CACHE_CONTROL,"no-store".parse().unwrap());
        response.headers_mut().insert("x-content-type-options","nosniff".parse().unwrap());
        response.headers_mut().insert("referrer-policy","no-referrer".parse().unwrap());
        response
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {token:String}
async fn login(State(d):State<Deployment>,Json(input):Json<Login>)->Response {
    if !d.matches(&input.token) {return StatusCode::UNAUTHORIZED.into_response();}
    ([(header::SET_COOKIE,format!("__Host-talia={}; Secure; HttpOnly; SameSite=Strict; Path=/; Max-Age=28800",d.token))],StatusCode::NO_CONTENT).into_response()
}
async fn asset(State(d):State<Deployment>,r:Request)->Response {
    let path=if r.uri().path()=="/" {"/dashboard/web/index.html"}else{r.uri().path()};
    if path=="/dashboard/web/index.html" && !d.authorized(&r) {
        return ([(header::CONTENT_TYPE,"text/html; charset=utf-8")],include_str!("../../deploy/login.html")).into_response();
    }
    // The packaged root contains only selected web assets; canonicalization also
    // rejects traversal and symlinks escaping that directory.
    if path.contains('%') || path.contains('\\') || path.split('/').any(|s|s==".."||s.starts_with('.')) {return StatusCode::NOT_FOUND.into_response();}
    let Ok(file)=d.root.join(path.trim_start_matches('/')).canonicalize() else {return StatusCode::NOT_FOUND.into_response()};
    if !file.starts_with(&d.root) || !file.is_file(){return StatusCode::NOT_FOUND.into_response();}
    let mime=match file.extension().and_then(|x|x.to_str()) {Some("html")=>"text/html; charset=utf-8",Some("js")=>"text/javascript",Some("css")=>"text/css",Some("svg")=>"image/svg+xml",Some("wasm")=>"application/wasm",_=>return StatusCode::NOT_FOUND.into_response()};
    match tokio::fs::read(file).await {Ok(bytes)=>Response::builder().header(header::CONTENT_TYPE,mime).body(Body::from(bytes)).unwrap(),Err(_)=>StatusCode::NOT_FOUND.into_response()}
}
