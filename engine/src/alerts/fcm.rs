//! FCM HTTP v1 with service-account OAuth. Keys are read from operator files only.
use super::{delivery::*, *};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use ring::{rand::SystemRandom, signature::{RsaKeyPair, RSA_PKCS1_SHA256}};
use std::time::{SystemTime, UNIX_EPOCH};
#[derive(Deserialize)]
struct Account {
    client_email: String,
    private_key: String,
    #[serde(default = "oauth_url")]
    token_uri: String,
}
fn oauth_url() -> String {
    "https://oauth2.googleapis.com/token".into()
}
pub fn base_url() -> String {
    "https://fcm.googleapis.com".into()
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn jwt(account: &Account) -> Result<String> {
    let pem = account
        .private_key
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<String>();
    let der = STANDARD
        .decode(pem)
        .map_err(|_| "invalid service account key")?;
    let key = RsaKeyPair::from_pkcs8(&der).map_err(|_| "invalid service account key")?;
    let issued = now() / 1000;
    let body=format!("{}.{}",URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#),URL_SAFE_NO_PAD.encode(json!({"iss":account.client_email,"scope":"https://www.googleapis.com/auth/firebase.messaging","aud":account.token_uri,"iat":issued,"exp":issued+3600}).to_string()));
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        body.as_bytes(),
        &mut signature,
    )
    .map_err(|_| "service account signing failed")?;
    Ok(format!("{body}.{}", URL_SAFE_NO_PAD.encode(signature)))
}
async fn body(mut response: reqwest::Response) -> Result<(u16, Value)> {
    let status = response.status().as_u16();
    let mut bytes = vec![];
    while let Some(part) = response
        .chunk()
        .await
        .map_err(|_| "provider response lost")?
    {
        if bytes.len() + part.len() > 65536 {
            return Err("provider response limit".into());
        }
        bytes.extend(part);
    }
    Ok((
        status,
        serde_json::from_slice(&bytes).map_err(|_| "invalid provider response")?,
    ))
}
pub async fn send(project: &str, account_path: &str, url: &str, d: &Dispatch) -> Outcome {
    match send_result(project, account_path, url, d).await {
        Ok(outcome) => outcome,
        Err(e) => Outcome::Unknown(e),
    }
}
async fn send_result(project: &str, path: &str, url: &str, d: &Dispatch) -> Result<Outcome> {
    use tokio::io::AsyncReadExt;
    if d.destination.channel != "push"
        || project.is_empty()
        || !project
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !super::providers::url_valid(url)
    {
        return Ok(Outcome::Failed("invalid FCM configuration".into()));
    }
    let mut bytes = vec![];
    tokio::fs::File::open(path)
        .await
        .map_err(|_| "service account unavailable")?
        .take(65537)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "service account unavailable")?;
    if bytes.len() > 65536 {
        return Err("service account size limit".into());
    }
    let account: Account = serde_json::from_slice(&bytes).map_err(|_| "invalid service account")?;
    if !super::providers::url_valid(&account.token_uri) {
        return Err("invalid OAuth endpoint".into());
    }
    let assertion = jwt(&account)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "HTTP transport unavailable")?;
    let response = client
        .post(&account.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ])
        .send()
        .await
        .map_err(|_| "OAuth endpoint unavailable")?;
    let (status, token) = body(response).await?;
    if status != 200 {
        return Ok(Outcome::Failed("FCM authorization rejected".into()));
    }
    let token = token["access_token"]
        .as_str()
        .ok_or("OAuth access token missing")?;
    let ttl = d.delivery.expires.saturating_sub(now()).max(0) / 1000;
    if ttl == 0 {
        return Ok(Outcome::Failed("push expired".into()));
    }
    let data = json!({"key":d.alert.key,"occurrence":d.alert.occurrence.to_string(),"revision":d.alert.revision.to_string(),"expires":d.delivery.expires.to_string(),"message":d.alert.message.chars().take(800).collect::<String>(),"severity":d.alert.severity,"active":d.alert.active.to_string(),"deliveryId":d.delivery.id});
    let response=client.post(format!("{}/v1/projects/{project}/messages:send",url.trim_end_matches('/'))).bearer_auth(token).json(&json!({"message":{"token":d.address,"data":data,"android":{"priority":"HIGH","ttl":format!("{ttl}s")}}})).send().await.map_err(|_|"FCM acceptance unknown")?;
    let (status, result) = body(response).await?;
    Ok(
        if status == 200 && result["name"].as_str().is_some_and(|s| !s.is_empty()) {
            Outcome::Sent
        } else if status == 429 || status >= 500 {
            Outcome::Retry("FCM temporary rejection".into())
        } else if (400..500).contains(&status) {
            Outcome::Failed("FCM rejected message or device token".into())
        } else {
            Outcome::Unknown("FCM acceptance not confirmed".into())
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn signed_oauth_and_fcm_payload_acceptance() {
        use axum::{routing::post, Json, Router};
        let root = std::env::temp_dir().join(format!("talia-fcm-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let key = root.join("key.pem");
        assert!(std::process::Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
                "-out"
            ])
            .arg(&key)
            .output()
            .unwrap()
            .status
            .success());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let account = root.join("account.json");
        std::fs::write(&account,json!({"client_email":"fixture@example.test","private_key":std::fs::read_to_string(&key).unwrap(),"token_uri":format!("{url}/token")}).to_string()).unwrap();
        let router = Router::new()
            .route(
                "/token",
                post(|body: String| async move {
                    assert!(body.contains("assertion="));
                    Json(json!({"access_token":"fixture-access"}))
                }),
            )
            .route(
                "/v1/projects/fixture/messages:send",
                post(
                    |headers: axum::http::HeaderMap, Json(v): Json<Value>| async move {
                        assert_eq!(headers["authorization"], "Bearer fixture-access");
                        assert_eq!(v["message"]["data"]["key"], "disk");
                        assert_eq!(v["message"]["token"], "device-token");
                        Json(json!({"name":"projects/fixture/messages/accepted"}))
                    },
                ),
            );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut s = super::super::delivery::tests::fixture();
        s.alert_schedule(0).unwrap();
        let j = s.alert_deliveries().unwrap().remove(0);
        let mut d = s.alert_claim_delivery(&j.id, 0).unwrap().unwrap();
        d.destination.channel = "push".into();
        d.address = "device-token".into();
        d.delivery.expires = now() + 60000;
        assert!(matches!(
            send("fixture", account.to_str().unwrap(), &url, &d).await,
            Outcome::Sent
        ));
        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }
}
