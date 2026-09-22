//! Streamable HTTP MCP. Every request is authenticated independently of transport sessions.
use crate::{deployment::Deployment, Request};
use axum::{
    extract::State,
    http::{request::Parts, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Router,
};
use rmcp::{
    model::*,
    service::RequestContext,
    transport::streamable_http_server::{
        session::never::NeverSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    },
    RoleServer, ServerHandler,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::{mpsc, oneshot, Semaphore};
#[derive(Clone)]
struct Auth {
    token: String,
    connection: String,
    admin: bool,
}
#[derive(Clone)]
struct StateData {
    tx: mpsc::Sender<Request>,
    deployment: Deployment,
    budget: Arc<Semaphore>,
}
async fn send(
    tx: &mpsc::Sender<Request>,
    token: &str,
    connection: &str,
    call: &str,
    body: Value,
) -> Value {
    let (reply, rx) = oneshot::channel();
    if tx
        .try_send(Request {
            http_mcp: true,
            browser_subject: None,
            body,
            reply,
            credential: Some(token.into()),
            agent: true,
            connection: connection.into(),
            call: call.into(),
        })
        .is_err()
    {
        return json!({"error":"limit_exceeded"});
    }
    rx.await
        .unwrap_or_else(|_| json!({"error":"internal_error"}))
}
async fn authenticate(state: &StateData, token: &str) -> Result<Value, StatusCode> {
    let info = send(&state.tx, token, "", "", json!({"name":"_key_auth"})).await;
    if info["error"] == "limit_exceeded" {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let session = info["authSession"]
        .as_str()
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let identity = state.deployment.identity_id(session).await?;
    if info["subject"] != identity.subject {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(info)
}
async fn gate(
    State(state): State<StateData>,
    mut req: axum::extract::Request,
    next: Next,
) -> Response {
    let Ok(_permit) = state.budget.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    // No browser-cookie authentication or token-in-URL fallback on this endpoint.
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_string();
    let info = match authenticate(&state, &token).await {
        Ok(i) => i,
        Err(e) => return (e, [("www-authenticate", "Bearer realm=\"talia\"")]).into_response(),
    };
    req.extensions_mut().insert(Auth {
        token: token.clone(),
        connection: format!("http-{}", &info["id"].as_str().unwrap()[..48]),
        admin: info["admin"] == true,
    });
    let mut response = next.run(req).await;
    // Do not release delayed data after expiry, revocation or provider-session revocation.
    match authenticate(&state, &token).await {
        Err(e) => return e.into_response(),
        Ok(current) if info["admin"] == true && current["admin"] != true => {
            return StatusCode::FORBIDDEN.into_response()
        }
        _ => {}
    }
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}
#[derive(Clone)]
struct Adapter {
    tx: mpsc::Sender<Request>,
    sequence: Arc<AtomicU64>,
}
fn auth(context: &RequestContext<RoleServer>) -> Result<Auth, ErrorData> {
    context
        .extensions
        .get::<Parts>()
        .and_then(|p| p.extensions.get::<Auth>())
        .cloned()
        .ok_or_else(|| ErrorData::invalid_request("Authentication required", None))
}
impl ServerHandler for Adapter {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info.name = "talia".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info.instructions = Some(talia_engine::mcp::INSTRUCTIONS.into());
        info
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.and_then(|r| r.cursor).is_some() {
            return Err(ErrorData::invalid_params("No cursor supported", None));
        }
        let a = auth(&context)?;
        Ok(serde_json::from_value(
            json!({"tools":if a.admin {talia_engine::mcp::tools()}else{vec![]}}),
        )
        .unwrap())
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let a = auth(&context)?;
        if !a.admin {
            return Err(ErrorData::invalid_request(
                "Viewer accounts cannot use authoring or engine tools",
                None,
            ));
        }
        if !talia_engine::mcp::tools()
            .iter()
            .any(|t| t["name"] == request.name.as_ref())
        {
            return Err(ErrorData::invalid_params("Unknown tool", None));
        }
        let call = format!(
            "http-call-{}",
            self.sequence.fetch_add(1, Ordering::Relaxed)
        );
        let value = send(
            &self.tx,
            &a.token,
            &a.connection,
            &call,
            json!({"name":request.name,"arguments":request.arguments.unwrap_or_default()}),
        )
        .await;
        let failed = !value["error"].is_null() || value.get("valid") == Some(&Value::Bool(false));
        Ok(serde_json::from_value::<CallToolResult>(json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":failed})).unwrap().into())
    }
}
pub fn routes(tx: mpsc::Sender<Request>, deployment: Deployment) -> Router {
    let state = StateData {
        tx: tx.clone(),
        deployment,
        budget: Arc::new(Semaphore::new(64)),
    };
    let sequence = Arc::new(AtomicU64::new(1));
    let origin = std::env::var("TALIA_PUBLIC_ORIGIN").expect("deployment origin");
    let url = url::Url::parse(&origin).expect("validated origin");
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap()),
        None => url.host_str().unwrap().into(),
    };
    let mut config = StreamableHttpServerConfig::default();
    config.legacy_session_mode = false;
    config.json_response = true;
    config.allowed_hosts = vec![host];
    config.allowed_origins = vec![origin];
    config.max_request_body_bytes = 2_359_296;
    let service = StreamableHttpService::new(
        move || {
            Ok(Adapter {
                tx: tx.clone(),
                sequence: sequence.clone(),
            })
        },
        Arc::new(NeverSessionManager::default()),
        config,
    );
    Router::new()
        .route_service("/mcp", service)
        .layer(axum::middleware::from_fn_with_state(state, gate))
}
