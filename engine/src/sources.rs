//! Bounded HTTP adapters. Script capability arguments never contain credentials.
use crate::{monitoring::DataSource, store::Result, value};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{rc::Rc, time::Duration};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceRequest {
    Query {
        query: String,
        #[serde(default)]
        time: Option<f64>,
    },
    Range {
        query: String,
        start: f64,
        end: f64,
        step: f64,
    },
    Http {
        #[serde(default)]
        path: String,
        #[serde(default = "get")]
        method: String,
        #[serde(default)]
        body: Option<Value>,
        #[serde(default)]
        text: bool,
    },
}
fn get() -> String {
    "GET".into()
}
impl SourceRequest {
    pub fn has_effect(&self) -> bool {
        matches!(self,Self::Http{method,..} if method!="GET"&&method!="HEAD")
    }
}
#[derive(Clone)]
pub struct Adapters {
    client: reqwest::Client,
    secrets: Rc<dyn Fn(&str) -> Option<String>>,
}
impl Adapters {
    pub fn new() -> Result<Self> {
        Self::with_secrets(Rc::new(|id| {
            std::env::var(format!("TALIA_SECRET_{id}")).ok()
        }))
    }
    pub fn with_secrets(secrets: Rc<dyn Fn(&str) -> Option<String>>) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .map_err(|_| "HTTP client configuration")?,
            secrets,
        })
    }
    pub async fn fetch(&self, source: &DataSource, request: &SourceRequest) -> Result<Value> {
        // Timeout includes connect, headers, all body chunks and decoding.
        tokio::time::timeout(
            Duration::from_millis(source.timeout_ms),
            self.fetch_inner(source, request),
        )
        .await
        .map_err(|_| "source timeout".to_string())?
    }
    async fn fetch_inner(&self, source: &DataSource, request: &SourceRequest) -> Result<Value> {
        let base = url::Url::parse(&source.url).map_err(|_| "source URL")?;
        let mut u = base.clone();
        let mut method = reqwest::Method::GET;
        let mut body = None;
        match request {
            SourceRequest::Query { query, time } => {
                if source.kind != "prometheus"
                    || query.is_empty()
                    || query.len() > 8192
                    || time.is_some_and(|x| !x.is_finite())
                {
                    return Err("invalid instant query".into());
                }
                u.set_path(&format!(
                    "{}/api/v1/query",
                    base.path().trim_end_matches('/')
                ));
                u.query_pairs_mut().append_pair("query", query);
                if let Some(t) = time {
                    u.query_pairs_mut().append_pair("time", &t.to_string());
                }
            }
            SourceRequest::Range {
                query,
                start,
                end,
                step,
            } => {
                if source.kind != "prometheus"
                    || query.is_empty()
                    || query.len() > 8192
                    || !start.is_finite()
                    || !end.is_finite()
                    || !step.is_finite()
                    || start > end
                    || *step <= 0.0
                    || (end - start) / step > 11000.0
                {
                    return Err("invalid/bounded range query".into());
                }
                u.set_path(&format!(
                    "{}/api/v1/query_range",
                    base.path().trim_end_matches('/')
                ));
                u.query_pairs_mut()
                    .append_pair("query", query)
                    .append_pair("start", &start.to_string())
                    .append_pair("end", &end.to_string())
                    .append_pair("step", &step.to_string());
            }
            SourceRequest::Http {
                path,
                method: m,
                body: b,
                ..
            } => {
                if source.kind != "http"
                    || path.len() > 8192
                    || !["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"].contains(&m.as_str())
                {
                    return Err("invalid HTTP probe".into());
                }
                u = base.join(path).map_err(|_| "probe path")?;
                if u.origin() != base.origin()
                    || !u.username().is_empty()
                    || u.password().is_some()
                    || u.fragment().is_some()
                {
                    return Err("probe must stay on configured origin".into());
                }
                method = reqwest::Method::from_bytes(m.as_bytes()).map_err(|_| "probe method")?;
                body = b.clone();
                if body
                    .as_ref()
                    .is_some_and(|v| serde_json::to_vec(v).map_or(true, |b| b.len() > 131072))
                {
                    return Err("request body limit".into());
                }
            }
        }
        let secret = match &source.credential_ref {
            Some(id) => Some(
                (self.secrets)(id)
                    .filter(|s| !s.is_empty())
                    .ok_or("credential unavailable")?,
            ),
            None => None,
        };
        let mut req = self
            .client
            .request(method, u)
            .timeout(Duration::from_millis(source.timeout_ms));
        if let Some(token) = &secret {
            req = req.bearer_auth(token);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let mut response = req
            .send()
            .await
            .map_err(|_| "source connection/request failed")?;
        if !response.status().is_success() {
            return Err(format!("source HTTP {}", response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|n| n > source.max_bytes as u64)
        {
            return Err("source response size limit".into());
        }
        let mut bytes = vec![];
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "source response interrupted")?
        {
            if bytes.len().saturating_add(chunk.len()) > source.max_bytes {
                return Err("source response size limit".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let as_text = matches!(request, SourceRequest::Http { text: true, .. });
        let mut parsed = if as_text {
            json!(String::from_utf8(bytes).map_err(|_| "source UTF-8")?)
        } else {
            serde_json::from_slice(&bytes).map_err(|_| "source invalid JSON")?
        };
        if let Some(secret) = secret {
            scrub(&mut parsed, &secret);
        }
        let wire = if source.kind == "prometheus" {
            prometheus(&parsed)?
        } else {
            value::from_json(&parsed)
        };
        value::validate(&wire)?;
        Ok(wire)
    }
}
fn scrub(v: &mut Value, secret: &str) {
    match v {
        Value::String(s) => *s = s.replace(secret, "[redacted]"),
        Value::Array(a) => {
            for v in a {
                scrub(v, secret)
            }
        }
        Value::Object(o) => {
            let old = std::mem::take(o);
            for (k, mut v) in old {
                scrub(&mut v, secret);
                o.insert(k.replace(secret, "[redacted]"), v);
            }
        }
        _ => {}
    }
}
fn sample(v: &Value, string: bool) -> Result<Value> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or("invalid Prometheus sample")?;
    let timestamp = a[0]
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or("invalid sample time")?;
    let raw = a[1].as_str().ok_or("invalid sample value")?;
    let n = if string {
        value::from_json(&json!(raw))
    } else {
        value::number(match raw {
            "+Inf" | "Inf" => f64::INFINITY,
            "-Inf" => f64::NEG_INFINITY,
            "NaN" => f64::NAN,
            _ => raw.parse::<f64>().map_err(|_| "invalid sample number")?,
        })
    };
    Ok(json!([
        "array",
        [value::number(timestamp)["value"], n["value"]]
    ]))
}
/// Preserve labels and sample timestamps; parse only numeric sample strings, never labels.
pub fn prometheus(v: &Value) -> Result<Value> {
    if v["status"] != "success" {
        return Err("Prometheus query failed".into());
    }
    let d = &v["data"];
    let kind = d["resultType"].as_str().ok_or("Prometheus result type")?;
    let result = match kind {
        "scalar" | "string" => sample(&d["result"], kind == "string")?,
        "vector" | "matrix" => {
            let mut series = vec![];
            for s in d["result"].as_array().ok_or("Prometheus series")? {
                let labels = s["metric"].as_object().ok_or("Prometheus labels")?;
                if labels.values().any(|v| !v.is_string()) {
                    return Err("Prometheus label value".into());
                }
                if s.get("histogram").is_some() || s.get("histograms").is_some() {
                    return Err("native histogram samples are not supported".into());
                }
                let samples = if kind == "vector" {
                    vec![sample(&s["value"], false)?]
                } else {
                    s["values"]
                        .as_array()
                        .ok_or("Prometheus samples")?
                        .iter()
                        .map(|x| sample(x, false))
                        .collect::<Result<Vec<_>>>()?
                };
                series.push(json!([
                    "object",
                    [
                        ["metric", value::from_json(&s["metric"])["value"]],
                        ["samples", ["array", samples]]
                    ]
                ]));
            }
            json!(["array", series])
        }
        _ => return Err("unsupported Prometheus result type".into()),
    };
    Ok(
        json!({"version":1,"value":["object",[["resultType",["string",kind]],["result",result],["warnings",value::from_json(v.get("warnings").unwrap_or(&json!([])))["value"]],["infos",value::from_json(v.get("infos").unwrap_or(&json!([])))["value"]]]]}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::HeaderMap, routing::get, Router};
    async fn fixture() -> (DataSource, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app=Router::new().route("/echo",get(|h:HeaderMap|async move {axum::Json(json!({"auth":h["authorization"].to_str().unwrap(),"cpu":42}))}))
   .route("/slow",get(||async {tokio::time::sleep(Duration::from_millis(100)).await;"{}"}))
   .route("/big",get(||async{"x".repeat(1024)}))
   .route("/bad",get(||async{(axum::http::StatusCode::BAD_GATEWAY,"secret error body")}))
   .route("/api/v1/query",get(||async{axum::Json(json!({"status":"success","data":{"resultType":"vector","result":[{"metric":{"service":"one"},"value":[123,"+Inf"]}]}}))}))
   .route("/api/v1/query_range",get(||async{axum::Json(json!({"status":"success","data":{"resultType":"matrix","result":[{"metric":{},"values":[[100,"NaN"],[200,"-Inf"]]}]}}))}));
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            DataSource {
                id: "fixture".into(),
                kind: "http".into(),
                url,
                credential_ref: Some("token".into()),
                timeout_ms: 1000,
                max_bytes: 8192,
            },
            task,
        )
    }
    #[tokio::test]
    async fn bounded_transport_and_lossless_results() {
        let (mut s, task) = fixture().await;
        let a = Adapters::with_secrets(Rc::new(|_| Some("fixture-secret".into()))).unwrap();
        let request = |path: &str| SourceRequest::Http {
            path: path.into(),
            method: "GET".into(),
            body: None,
            text: false,
        };
        let r = a.fetch(&s, &request("/echo")).await.unwrap();
        assert!(!r.to_string().contains("fixture-secret"));
        assert!(r.to_string().contains("[redacted]"));
        assert!(a.fetch(&s, &request("https://example.com/")).await.is_err());
        assert_eq!(
            a.fetch(&s, &request("/bad")).await.unwrap_err(),
            "source HTTP 502"
        );
        s.max_bytes = 32;
        assert!(a
            .fetch(&s, &request("/big"))
            .await
            .unwrap_err()
            .contains("size limit"));
        s.max_bytes = 8192;
        s.timeout_ms = 10;
        assert!(a.fetch(&s, &request("/slow")).await.is_err());
        s.timeout_ms = 1000;
        s.kind = "prometheus".into();
        let r = a
            .fetch(
                &s,
                &SourceRequest::Query {
                    query: "up".into(),
                    time: None,
                },
            )
            .await
            .unwrap();
        assert!(r.to_string().contains("Infinity"));
        value::validate(&r).unwrap();
        let r = a
            .fetch(
                &s,
                &SourceRequest::Range {
                    query: "up".into(),
                    start: 100.0,
                    end: 200.0,
                    step: 10.0,
                },
            )
            .await
            .unwrap();
        assert!(r.to_string().contains("NaN") && r.to_string().contains("-Infinity"));
        task.abort();
    }
}
