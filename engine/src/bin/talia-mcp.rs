//! Stdio MCP adapter. The running engine owns credentials, policy, storage and audits.
use rmcp::{model::*, service::RequestContext, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use talia_engine::mcp;
/// The SDK's default stdio reader has no line-size cap. Bound it before JSON parsing.
struct BoundedLines<R> {
    inner: R,
    length: usize,
}
impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for BoundedLines<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let start = buf.filled().len();
        match std::pin::Pin::new(&mut self.inner).poll_read(cx, buf) {
            std::task::Poll::Ready(Ok(())) => {
                for byte in &buf.filled()[start..] {
                    if *byte == b'\n' {
                        self.length = 0;
                    } else {
                        self.length += 1;
                    }
                    if self.length > 2_359_296 {
                        // AsyncRead must not advance the caller buffer when returning an error.
                        buf.set_filled(start);
                        return std::task::Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "MCP frame limit",
                        )));
                    }
                }
                std::task::Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
}
struct Adapter {
    client: reqwest::Client,
    endpoint: url::Url,
    token: String,
    active: tokio::sync::Semaphore,
    connection: String,
    sequence: std::sync::atomic::AtomicU64,
}
fn tool_result(value: Value) -> CallToolResponse {
    let failed = !value["error"].is_null() || value.get("valid") == Some(&Value::Bool(false));
    serde_json::from_value::<CallToolResult>(json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":failed})).expect("tool response shape").into()
}
impl Adapter {
    async fn control(&self, name: &str, arguments: Value) {
        let request = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.token)
            .header("x-talia-session", &self.connection)
            .json(&json!({"name":name,"arguments":arguments}))
            .send();
        let _ = tokio::time::timeout(Duration::from_secs(2), request).await;
    }
}
impl ServerHandler for Adapter {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info.name = "talia".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info.instructions = Some(mcp::INSTRUCTIONS.into());
        info
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if request.and_then(|r| r.cursor).is_some() {
            return Err(ErrorData::invalid_params(
                "No tools cursor is supported",
                None,
            ));
        }
        Ok(serde_json::from_value(json!({"tools":mcp::tools()})).expect("tool schemas"))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !mcp::tools()
            .iter()
            .any(|t| t["name"] == request.name.as_ref())
        {
            return Err(ErrorData::invalid_params("Unknown tool", None));
        }
        let Ok(_permit) = self.active.try_acquire() else {
            return Ok(tool_result(json!({"error":"limit_exceeded"})));
        };
        let body = json!({"name":request.name,"arguments":request.arguments.unwrap_or_default()});
        if body.to_string().len() > 2_097_152 {
            return Ok(tool_result(json!({"error":"limit_exceeded"})));
        }
        let call = format!(
            "call-{}",
            self.sequence
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let work = async {
            let mut response = self
                .client
                .post(self.endpoint.clone())
                .bearer_auth(&self.token)
                .header("x-talia-session", &self.connection)
                .header("x-talia-call", &call)
                .json(&body)
                .send()
                .await
                .map_err(|_| "target_unavailable")?;
            if !response.status().is_success() {
                return Err("target_unavailable");
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| "target_unavailable")? {
                if bytes.len() + chunk.len() > 2_359_296 {
                    return Err("limit_exceeded");
                }
                bytes.extend_from_slice(&chunk);
            }
            serde_json::from_slice::<Value>(&bytes).map_err(|_| "internal_error")
        };
        // Cancelling the wait never undoes an admitted mutation. Query its requestId afterwards.
        let result = tokio::select! { biased; _=context.ct.cancelled()=>{self.control("_cancel_call",json!({"callId":call})).await;Err("cancelled")}, result=work=>result };
        Ok(tool_result(result.unwrap_or_else(|code|json!({"error":code,"recovery":"If a mutation was submitted, use operation_status with its requestId; do not assume effects were undone."}))))
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: talia-mcp http://127.0.0.1:PORT /path/to/credential".into());
    }
    let mut endpoint = url::Url::parse(&args[1]).map_err(|_| "invalid engine URL")?;
    if endpoint.scheme() != "http"
        || endpoint.host_str() != Some("127.0.0.1")
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.path() != "/"
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err("engine URL must be an IPv4 loopback HTTP origin".into());
    }
    endpoint.set_path("/agent");
    let token = std::fs::read_to_string(&args[2]).map_err(|_| "cannot read credential file")?;
    let token = token.trim().to_string();
    if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid credential file".into());
    }
    let adapter = Adapter {
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()?,
        endpoint,
        token,
        active: tokio::sync::Semaphore::new(16),
        connection: talia_engine::mcp_engine::random_id()
            .map_err(|_| "session randomness unavailable")?,
        sequence: std::sync::atomic::AtomicU64::new(1),
    };
    let (input, output) = rmcp::transport::stdio();
    let adapter = Arc::new(adapter);
    let service = adapter
        .clone()
        .serve((
            BoundedLines {
                inner: input,
                length: 0,
            },
            output,
        ))
        .await?;
    let result = service.waiting().await;
    adapter.control("_session_close", json!({})).await;
    result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    #[tokio::test]
    async fn frame_bound_rejects_unterminated_input_and_resets_per_line() {
        let bytes = vec![b'x'; 2_359_297];
        let mut reader = BoundedLines {
            inner: bytes.as_slice(),
            length: 0,
        };
        assert!(reader.read_to_end(&mut Vec::new()).await.is_err());
        let mut lines = vec![b'x'; 2_359_296];
        lines.push(b'\n');
        lines.extend_from_slice(b"{}\n");
        let mut reader = BoundedLines {
            inner: lines.as_slice(),
            length: 0,
        };
        assert_eq!(
            reader.read_to_end(&mut Vec::new()).await.unwrap(),
            lines.len()
        );
    }
}
