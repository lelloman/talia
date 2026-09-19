//! Loopback-only experiment. In-memory state and explicit fault controls; not a deployment server.
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    session: String,
    channel: String,
    epoch: u64,
    id: u64,
    op: String,
    args: Value,
}
#[derive(Clone)]
struct Action {
    args: Value,
    status: &'static str,
    result: Value,
}
#[derive(Default)]
struct Engine {
    value: i64,
    revision: u64,
    actions: HashMap<String, Action>,
    held: HashSet<String>,
    entered: HashSet<String>,
    drop: HashSet<String>,
}
impl Engine {
    fn enter(&mut self, key: &str) -> Result<(), String> {
        if !self.entered.contains(key) && self.entered.len() >= 256 {
            return Err("test observation budget".into());
        }
        self.entered.insert(key.into());
        Ok(())
    }
    fn snapshot(&self) -> Value {
        json!({"value":self.value,"revision":self.revision})
    }
}
#[derive(Clone)]
struct Server {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<Engine>>>>>,
    slots: Arc<Semaphore>,
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}
fn fields(v: &Value, names: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == names.len() && names.iter().all(|k| o.contains_key(*k)))
}
fn token(v: &Value) -> Result<String, String> {
    let s = v.as_str().ok_or("identifier")?;
    if !identifier(s) {
        return Err("identifier".into());
    }
    Ok(s.into())
}
async fn gate(engine: &Arc<Mutex<Engine>>, key: &str) -> Result<(), String> {
    // Test-controlled latency, bounded even if the controlling client disappears.
    for _ in 0..1000 {
        if !engine.lock().unwrap().held.contains(key) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Err("test gate expired".into())
}
async fn execute(engine: Arc<Mutex<Engine>>, r: &Request) -> Result<Value, String> {
    let a = &r.args;
    match r.op.as_str() {
        "read" if fields(a, &["tag"]) => {
            let key = token(&a["tag"])?;
            // Capture before suspension, deliberately allowing a stale network response.
            let snapshot = {
                let mut e = engine.lock().unwrap();
                e.enter(&key)?;
                e.snapshot()
            };
            gate(&engine, &key).await?;
            Ok(snapshot)
        }
        "watch" if fields(a, &["after"]) && a["after"].as_u64().is_some() => {
            let after = a["after"].as_u64().unwrap();
            for _ in 0..40 {
                let snapshot = engine.lock().unwrap().snapshot();
                if snapshot["revision"].as_u64().unwrap() > after {
                    return Ok(snapshot);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            // Snapshot resync, not a historical event stream. Intermediate values may coalesce.
            Ok(engine.lock().unwrap().snapshot())
        }
        "action" if fields(a, &["actionId", "value"]) => {
            let id = token(&a["actionId"])?;
            let value = a["value"]
                .as_i64()
                .filter(|v| (-1_000_000..=1_000_000).contains(v))
                .ok_or("value range")?;
            {
                let mut e = engine.lock().unwrap();
                e.enter(&id)?;
                if let Some(old) = e.actions.get(&id) {
                    if old.args != *a {
                        return Err("action ID conflict".into());
                    }
                } else {
                    if e.actions.len() >= 128 {
                        return Err("action budget".into());
                    }
                    e.actions.insert(
                        id.clone(),
                        Action {
                            args: a.clone(),
                            status: "accepted",
                            result: Value::Null,
                        },
                    );
                }
            }
            // Spawned RPC execution survives loss of the requesting HTTP connection.
            if let Err(error) = gate(&engine, &id).await {
                let mut e = engine.lock().unwrap();
                let action = e.actions.get_mut(&id).unwrap();
                if action.status == "accepted" {
                    action.status = "failed";
                    action.result = json!({"error":error});
                }
            }
            let outcome = {
                let mut e = engine.lock().unwrap();
                if e.actions[&id].status == "accepted" {
                    e.value = value;
                    e.revision += 1;
                    let snapshot = e.snapshot();
                    let action = e.actions.get_mut(&id).unwrap();
                    action.status = "completed";
                    action.result = snapshot;
                }
                let action = &e.actions[&id];
                json!({"status":action.status,"result":action.result})
            };
            // A reply delay expiring cannot turn a committed effect into a failed action.
            let _ = gate(&engine, &format!("{id}-reply")).await;
            Ok(outcome)
        }
        "status" | "cancel" if fields(a, &["actionId"]) => {
            let id = token(&a["actionId"])?;
            let mut e = engine.lock().unwrap();
            if let Some(action) = e.actions.get_mut(&id) {
                if r.op == "cancel" && action.status == "accepted" {
                    action.status = "cancelled";
                }
                Ok(json!({"status":action.status,"result":action.result}))
            } else {
                Ok(json!({"status":"unknown","result":null}))
            }
        }
        // Trusted fixture controls; explicitly not production engine capabilities.
        "test" if fields(a, &["command", "key"]) => {
            let key = token(&a["key"])?;
            let mut e = engine.lock().unwrap();
            match a["command"].as_str() {
                Some("hold") if e.held.len() < 16 => {
                    e.held.insert(key);
                    Ok(json!(true))
                }
                Some("release") => {
                    e.held.remove(&key);
                    Ok(json!(true))
                }
                Some("entered") => Ok(json!(e.entered.contains(&key))),
                Some("drop") if e.drop.len() < 16 => {
                    e.drop.insert(key);
                    Ok(json!(true))
                }
                _ => Err("test control".into()),
            }
        }
        _ => Err("capability/arguments".into()),
    }
}
async fn rpc(State(server): State<Server>, Json(r): Json<Request>) -> Response {
    let permit = match server.slots.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return (StatusCode::TOO_MANY_REQUESTS, "request budget").into_response(),
    };
    if !identifier(&r.session)
        || !identifier(&r.channel)
        || r.epoch == 0
        || r.id == 0
        || r.id > 9_007_199_254_740_991
    {
        return (StatusCode::BAD_REQUEST, "envelope").into_response();
    }
    let engine = {
        let mut sessions = server.sessions.lock().unwrap();
        if !sessions.contains_key(&r.session) && sessions.len() >= 32 {
            return (StatusCode::TOO_MANY_REQUESTS, "session budget").into_response();
        }
        sessions.entry(r.session.clone()).or_default().clone()
    };
    let task = tokio::spawn(async move {
        let _permit = permit;
        let result = execute(engine.clone(), &r).await;
        let drop_reply = r.op == "action"
            && engine
                .lock()
                .unwrap()
                .drop
                .remove(r.args["actionId"].as_str().unwrap_or(""));
        let reply = match result {
            Ok(value) => json!({"channel":r.channel,"epoch":r.epoch,"id":r.id,"value":value}),
            Err(error) => json!({"channel":r.channel,"epoch":r.epoch,"id":r.id,"error":error}),
        };
        (reply, drop_reply)
    });
    match task.await {
        Ok((_, true)) => Response::new(Body::from_stream(
            futures_util::stream::once(async {
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"{"))
            })
            .chain(futures_util::stream::once(async {
                // Flush headers and partial body before dropping the connection, so
                // browser connection recovery cannot hide the lost response.
                tokio::time::sleep(Duration::from_millis(20)).await;
                Err::<Bytes, _>(std::io::Error::other(
                    "injected loss after server execution",
                ))
            })),
        )),
        Ok((reply, false)) => Json(reply).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "execution failed").into_response(),
    }
}
async fn asset(uri: axum::http::Uri) -> Response {
    let (path, mime) = match uri.path() {
        "/" => ("web/index.html", "text/html"),
        "/client.js" => ("shared/client.js", "application/javascript"),
        "/suite.js" => ("shared/suite.js", "application/javascript"),
        "/worker.js" => ("web/dist/worker.js", "application/javascript"),
        "/ng.wasm" => ("../runtime/web/dist/ng.wasm", "application/wasm"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    match tokio::task::spawn_blocking(move || std::fs::read(root.join(path)))
        .await
        .unwrap()
    {
        Ok(bytes) => ([("content-type", mime)], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
#[tokio::main]
async fn main() {
    let port = std::env::args()
        .nth(1)
        .unwrap_or("0".into())
        .parse::<u16>()
        .unwrap();
    let server = Server {
        sessions: Default::default(),
        slots: Arc::new(Semaphore::new(64)),
    };
    let app = Router::new()
        .route("/rpc", post(rpc))
        .fallback(get(asset))
        .layer(DefaultBodyLimit::max(32768))
        .with_state(server);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    println!("{}", json!({"port":listener.local_addr().unwrap().port()}));
    axum::serve(listener, app).await.unwrap();
}
