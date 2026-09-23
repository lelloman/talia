//! Operator-owned provider configuration is loaded asynchronously on each dispatch.
//! Neither URLs containing credentials nor remote error bodies are returned to guests.
use super::{delivery::*, *};
use crate::runtime::Engine;
use lettre::{
    transport::smtp::authentication::Credentials, AsyncSmtpTransport, AsyncTransport, Message,
    Tokio1Executor,
};
use std::{cell::Cell, rc::Rc, time::Duration};
fn telegram_base() -> String {
    "https://api.telegram.org".into()
}
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Provider {
    WebPush {
        private_key: String,
        subject: String,
        #[serde(default = "super::web_push::default_origins")]
        allowed_origins: Vec<String>,
    },
    Fcm {
        project: String,
        service_account: String,
        #[serde(default = "super::fcm::base_url")]
        base_url: String,
    },
    Smtp {
        host: String,
        port: u16,
        tls: String,
        from: String,
        #[serde(default)]
        username: Option<String>,
        #[serde(default)]
        password: Option<String>,
    },
    Telegram {
        token: String,
        #[serde(default = "telegram_base")]
        base_url: String,
    },
}
fn local(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}
pub(crate) fn url_valid(u: &str) -> bool {
    url::Url::parse(u).is_ok_and(|u| {
        u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && (u.scheme() == "https" || u.scheme() == "http" && u.host_str().is_some_and(local))
    })
}
pub async fn configuration(path: &std::path::Path) -> Result<BTreeMap<String, Provider>> {
    use tokio::io::AsyncReadExt;
    let f = tokio::fs::File::open(path)
        .await
        .map_err(|_| "provider configuration unavailable")?;
    let mut bytes = vec![];
    f.take(65537)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "provider configuration unavailable")?;
    if bytes.len() > 65536 {
        return Err("provider configuration too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid provider configuration".into())
}
pub async fn send(p: &Provider, dispatch: &Dispatch) -> Outcome {
    match tokio::time::timeout(Duration::from_secs(15), send_inner(p, dispatch)).await {
        Ok(v) => v,
        Err(_) => Outcome::Unknown("provider timeout; acceptance unknown".into()),
    }
}
async fn send_inner(p: &Provider, d: &Dispatch) -> Outcome {
    let state = if d.alert.active { "active" } else { "resolved" };
    let text = format!(
        "Talìa — {} [{}]\n{}\n{} (occurrence {})",
        d.alert.severity, state, d.alert.message, d.alert.key, d.alert.occurrence
    );
    match p {
        Provider::WebPush {private_key, subject, allowed_origins} => super::web_push::send(private_key, subject, allowed_origins, d).await,
        Provider::Fcm {
            project,
            service_account,
            base_url,
        } => super::fcm::send(project, service_account, base_url, d).await,
        Provider::Smtp {
            host,
            port,
            tls,
            from,
            username,
            password,
        } => {
            if d.destination.channel != "email" {
                return Outcome::Failed("provider/channel mismatch".into());
            }
            let transport = match tls.as_str() {
                "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
                "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
                "none_loopback" if local(host) => Ok(
                    AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
                ),
                _ => return Outcome::Failed("invalid SMTP TLS configuration".into()),
            };
            let Ok(mut transport) = transport else {
                return Outcome::Failed("invalid SMTP configuration".into());
            };
            transport = transport.port(*port).timeout(Some(Duration::from_secs(10)));
            match (username, password) {
                (Some(u), Some(p)) => {
                    transport = transport.credentials(Credentials::new(u.clone(), p.clone()))
                }
                (None, None) => (),
                _ => return Outcome::Failed("incomplete SMTP credentials".into()),
            }
            let Ok(from) = from.parse() else {
                return Outcome::Failed("invalid sender".into());
            };
            let Ok(to) = d.address.parse() else {
                return Outcome::Failed("invalid recipient".into());
            };
            let Ok(message) = Message::builder()
                .from(from)
                .to(to)
                .message_id(Some(format!("{}@talia", d.delivery.id)))
                .subject(format!("Talìa: {} ({state})", d.alert.severity))
                .body(text)
            else {
                return Outcome::Failed("invalid email".into());
            };
            match transport.build().send(message).await {
                Ok(_) => Outcome::Sent,
                Err(e) if e.is_permanent() => Outcome::Failed("SMTP rejected message".into()),
                Err(e) if e.is_transient() => Outcome::Retry("SMTP temporary rejection".into()),
                Err(_) => Outcome::Unknown("SMTP delivery outcome unknown".into()),
            }
        }
        Provider::Telegram { token, base_url } => {
            if d.destination.channel != "telegram" {
                return Outcome::Failed("provider/channel mismatch".into());
            }
            if !url_valid(base_url)
                || token.is_empty()
                || token.len() > 256
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
            {
                return Outcome::Failed("invalid Telegram configuration".into());
            }
            let Ok(client) = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .build()
            else {
                return Outcome::Failed("HTTP transport unavailable".into());
            };
            let response=client.post(format!("{}/bot{token}/sendMessage",base_url.trim_end_matches('/'))).json(&json!({"chat_id":d.address,"text":text.chars().take(4000).collect::<String>()})).send().await;
            let Ok(mut response) = response else {
                return Outcome::Unknown("Telegram delivery outcome unknown".into());
            };
            let status = response.status();
            let mut bytes = vec![];
            loop {
                match response.chunk().await {
                    Ok(Some(b)) => {
                        if bytes.len() + b.len() > 65536 {
                            return Outcome::Unknown("Telegram response exceeds limit".into());
                        }
                        bytes.extend(b);
                    }
                    Ok(None) => break,
                    Err(_) => return Outcome::Unknown("Telegram response lost".into()),
                }
            }
            let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            if status.is_success() && body["ok"] == true && body["result"]["message_id"].is_i64() {
                Outcome::Sent
            } else if status.as_u16() == 429
                || status.is_server_error()
                || body["error_code"] == 429
            {
                Outcome::RetryAfter(
                    "Telegram temporary rejection".into(),
                    body["parameters"]["retry_after"]
                        .as_u64()
                        .unwrap_or(1)
                        .saturating_mul(1000),
                )
            } else if status.is_client_error() || body["ok"] == false {
                Outcome::Failed("Telegram rejected message".into())
            } else {
                Outcome::Unknown("Telegram acceptance not confirmed".into())
            }
        }
    }
}
#[derive(Clone)]
pub struct Sender {
    engine: Engine,
    active: Rc<Cell<usize>>,
    config: Option<std::path::PathBuf>,
}
impl Sender {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            active: Default::default(),
            config: std::env::var_os("TALIA_ALERT_PROVIDERS").map(Into::into),
        }
    }
    pub fn tick(&self) -> Result<()> {
        for j in self.engine.store.borrow().alert_deliveries()? {
            if self.active.get() >= 8 {
                break;
            }
            if j.status != "pending" || j.due > self.engine.now() {
                continue;
            }
            // Claim in a separate statement: the immutable query borrow above must not span mutations.
            self.active.set(self.active.get() + 1);
            let this = self.clone();
            tokio::task::spawn_local(async move {
                let config = match &this.config {
                    Some(path) => configuration(path).await,
                    None => Err("provider configuration unavailable".into()),
                };
                let claim = this
                    .engine
                    .store
                    .borrow_mut()
                    .alert_claim_delivery(&j.id, this.engine.now());
                if let Ok(Some(dispatch)) = claim {
                    let managed = if dispatch.destination.provider == crate::telegram::PROVIDER && dispatch.destination.channel == "telegram" {
                        Some(match crate::telegram::transport::Bot::load(&this.engine).await {
                            Ok((_,bot))=>send(&Provider::Telegram{token:bot.token,base_url:bot.base},&dispatch).await,
                            Err(e)=>Outcome::Failed(e),
                        })
                    }else{None};
                    let outcome = if let Some(outcome)=managed {outcome}else{match config {
                        Ok(config) => match config.get(&dispatch.destination.provider) {
                            Some(p) => send(p, &dispatch).await,
                            None => Outcome::Failed("provider not configured".into()),
                        },
                        Err(e) => Outcome::Failed(e),
                    }};
                    let result = this.engine.store.borrow_mut().alert_finish_delivery(
                        &j.id,
                        outcome,
                        this.engine.now(),
                    );
                    if result.is_err() {
                        eprintln!("alert outcome persistence failed");
                    }
                }
                this.active.set(this.active.get() - 1);
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Router};
    async fn dispatch(channel: &str) -> Dispatch {
        let mut s = super::super::delivery::tests::fixture();
        s.alert_schedule(0).unwrap();
        let j = s.alert_deliveries().unwrap().remove(0);
        let mut d = s.alert_claim_delivery(&j.id, 0).unwrap().unwrap();
        d.destination.channel = channel.into();
        d.address = if channel == "email" {
            "user@example.test"
        } else {
            "123"
        }
        .into();
        d
    }
    #[tokio::test]
    async fn telegram_verifies_acceptance_and_sanitizes_failures() {
        for (status, body, expected) in [
            (200, r#"{"ok":true,"result":{"message_id":1}}"#, 0),
            (400, r#"{"ok":false,"description":"secret-token"}"#, 1),
            (429, r#"{"ok":false}"#, 2),
            (200, r#"{}"#, 3),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = tokio::spawn(async move {
                axum::serve(
                    listener,
                    Router::new().route(
                        "/bottest-token/sendMessage",
                        post(move || async move {
                            (axum::http::StatusCode::from_u16(status).unwrap(), body)
                        }),
                    ),
                )
                .await
                .unwrap()
            });
            let result = send(
                &Provider::Telegram {
                    token: "test-token".into(),
                    base_url: format!("http://127.0.0.1:{port}"),
                },
                &dispatch("telegram").await,
            )
            .await;
            let actual = match result {
                Outcome::Sent => 0,
                Outcome::Failed(ref s) => {
                    assert!(!s.contains("secret-token"));
                    1
                }
                Outcome::Retry(_) | Outcome::RetryAfter(_, _) => 2,
                Outcome::Unknown(_) => 3,
                Outcome::ExpiredAddress(_) => panic!("unexpected expired Telegram address"),
            };
            assert_eq!(actual, expected);
            server.abort();
        }
    }
    #[tokio::test]
    async fn smtp_success_rejection_and_loss() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        for mode in ["success", "reject", "lost"] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let (mut read, mut write) = socket.into_split();
                write.write_all(b"220 fixture ESMTP\r\n").await.unwrap();
                let mut reader = BufReader::new(&mut read);
                let mut data = false;
                let mut content = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap() == 0 {
                        break;
                    }
                    if data {
                        if line == ".\r\n" {
                            assert!(content.contains("disk low"));
                            if mode == "lost" {
                                break;
                            }
                            write
                                .write_all(if mode == "reject" {
                                    b"550 rejected\r\n"
                                } else {
                                    b"250 queued\r\n"
                                })
                                .await
                                .unwrap();
                            data = false;
                        } else {
                            content.push_str(&line);
                        }
                        continue;
                    }
                    let answer: &[u8] = if line.starts_with("EHLO") {
                        b"250 fixture\r\n".as_slice()
                    } else if line.starts_with("DATA") {
                        data = true;
                        b"354 send data\r\n"
                    } else if line.starts_with("QUIT") {
                        write.write_all(b"221 bye\r\n").await.unwrap();
                        break;
                    } else {
                        b"250 ok\r\n"
                    };
                    write.write_all(answer).await.unwrap();
                }
            });
            let result = send(
                &Provider::Smtp {
                    host: "127.0.0.1".into(),
                    port,
                    tls: "none_loopback".into(),
                    from: "talia@example.test".into(),
                    username: None,
                    password: None,
                },
                &dispatch("email").await,
            )
            .await;
            match (mode, result) {
                ("success", Outcome::Sent)
                | ("reject", Outcome::Failed(_))
                | ("lost", Outcome::Unknown(_)) => (),
                (_, r) => panic!("unexpected outcome: {r:?}"),
            };
            server.await.unwrap();
        }
    }
}
