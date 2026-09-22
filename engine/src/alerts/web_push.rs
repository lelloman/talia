//! RFC 8291 encryption and RFC 8292 VAPID using web-push; bounded reqwest transport.
use super::{delivery::*, providers::Provider, *};
use ::web_push::{ContentEncoding, SubscriptionInfo, VapidSignatureBuilder, WebPushMessageBuilder};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use tokio::io::AsyncReadExt;

pub fn default_origins() -> Vec<String> {
    vec![
        "https://fcm.googleapis.com".into(),
        "https://updates.push.services.mozilla.com".into(),
        "https://web.push.apple.com".into(),
    ]
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Address {
    pub subscription: SubscriptionInfo,
    pub application_server_key: String,
}
pub fn address(raw: &str) -> Result<Address> {
    let a: Address = serde_json::from_str(raw).map_err(|_| "invalid browser subscription")?;
    let u = url::Url::parse(&a.subscription.endpoint).map_err(|_| "invalid push endpoint")?;
    if raw.len() > 4096
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || !(u.scheme() == "https"
            || u.scheme() == "http"
                && u.host_str().is_some_and(|h| {
                    h.parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
                }))
    {
        return Err("invalid push endpoint".into());
    }
    for (text, len) in [
        (&a.subscription.keys.p256dh, 65),
        (&a.subscription.keys.auth, 16),
        (&a.application_server_key, 65),
    ] {
        let bytes = URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| "invalid push key")?;
        if bytes.len() != len || len == 65 && bytes[0] != 4 {
            return Err("invalid push key".into());
        }
    }
    Ok(a)
}
pub fn endpoint_allowed(endpoint: &str, allowed: &[String]) -> bool {
    let Ok(u) = url::Url::parse(endpoint) else {
        return false;
    };
    allowed.len() <= 32
        && allowed.iter().any(|o| {
            url::Url::parse(o).is_ok_and(|origin| {
                origin.origin().ascii_serialization() == *o && origin.origin() == u.origin()
            })
        })
}
async fn signing_key(path: &str) -> Result<::web_push::PartialVapidSignatureBuilder> {
    let f = tokio::fs::File::open(path)
        .await
        .map_err(|_| "Web Push key unavailable")?;
    let mut bytes = vec![];
    f.take(16385)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "Web Push key unavailable")?;
    if bytes.len() > 16384 {
        return Err("Web Push key too large".into());
    }
    VapidSignatureBuilder::from_pem_no_sub(bytes.as_slice())
        .map_err(|_| "invalid Web Push key".into())
}
pub async fn public_config() -> Result<Value> {
    let path =
        std::env::var("TALIA_ALERT_PROVIDERS").map_err(|_| "Browser push is not configured")?;
    let config = super::providers::configuration(std::path::Path::new(&path)).await?;
    for (id, p) in config {
        if let Provider::WebPush {
            private_key,
            allowed_origins,
            ..
        } = p
        {
            return Ok(
                json!({"provider":id,"applicationServerKey":URL_SAFE_NO_PAD.encode(signing_key(&private_key).await?.get_public_key()),"allowedOrigins":allowed_origins}),
            );
        }
    }
    Err("Browser push is not configured".into())
}
pub async fn send(path: &str, subject: &str, allowed: &[String], d: &Dispatch) -> Outcome {
    match prepare_send(path, subject, allowed, d).await {
        Ok(o) => o,
        Err(e) => Outcome::Failed(e),
    }
}
async fn prepare_send(
    path: &str,
    subject: &str,
    allowed: &[String],
    d: &Dispatch,
) -> Result<Outcome> {
    if d.destination.channel != "web_push" {
        return Err("provider/channel mismatch".into());
    }
    let a = address(&d.address)?;
    if !endpoint_allowed(&a.subscription.endpoint, allowed) {
        return Err("push endpoint origin is not allowed".into());
    }
    if !(subject.starts_with("mailto:")
        || url::Url::parse(subject).is_ok_and(|u| u.scheme() == "https"))
        || subject.len() > 512
    {
        return Err("invalid VAPID subject".into());
    }
    let key = signing_key(path).await?;
    if URL_SAFE_NO_PAD.encode(key.get_public_key()) != a.application_server_key {
        return Err("browser must re-enroll after Web Push key rotation".into());
    }
    let mut signature = key.add_sub_info(&a.subscription);
    signature.add_claim("sub", subject);
    let signature = signature.build().map_err(|_| "VAPID signing failed")?;
    // No alert text or credentials in transit payload. The worker obtains current
    // details through the signed-in owner's authorized API, including after logout.
    let payload=json!({"version":1,"device":d.delivery.device,"key":d.alert.key,"occurrence":d.alert.occurrence,"revision":d.alert.revision,"expires":d.delivery.expires}).to_string();
    let now = chrono::Utc::now().timestamp_millis();
    if d.delivery.expires <= now {
        return Err("push expired".into());
    }
    let mut builder = WebPushMessageBuilder::new(&a.subscription);
    builder.set_payload(ContentEncoding::Aes128Gcm, payload.as_bytes());
    builder.set_vapid_signature(signature);
    builder.set_ttl(((d.delivery.expires - now) / 1000).clamp(0, 86400) as u32);
    let message = builder.build().map_err(|_| "push encryption failed")?;
    let request = ::web_push::request_builder::build_request::<Vec<u8>>(message);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "push transport unavailable")?;
    let mut send = client.post(request.uri().to_string());
    for (k, v) in request.headers() {
        send = send.header(k.as_str(), v.as_bytes());
    }
    let response = match send.body(request.into_body()).send().await {
        Ok(r) => r,
        Err(_) => return Ok(Outcome::Unknown("Web Push acceptance unknown".into())),
    };
    let status = response.status().as_u16();
    Ok(match status {
        201 | 202 => Outcome::Sent,
        404 | 410 => Outcome::ExpiredAddress(d.address.clone()),
        429 | 500..=599 => Outcome::RetryAfter(
            "Web Push temporary rejection".into(),
            response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1)
                .min(86400)
                * 1000,
        ),
        _ => Outcome::Failed("Web Push rejected delivery".into()),
    })
}

impl Store {
    pub fn browser_push_register(
        &mut self,
        owner: &str,
        id: &str,
        token: &str,
        provider: &str,
        now: i64,
    ) -> Result<Value> {
        if id.len() != 44
            || !id.starts_with("browser-")
            || !id[8..].bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        {
            return Err("invalid browser ID".into());
        }
        self.alert_atomic(|s| {
            let status = s.browser_push_status(owner, id)?;
            let expected = status["device"]["version"].as_u64().unwrap_or(0);
            let old = s.alert_destinations()?.into_iter().find(|d| d.id == id);
            if old
                .as_ref()
                .is_some_and(|d| d.target != format!("device:{id}") || d.channel != "web_push")
            {
                return Err("destination ID conflict".into());
            }
            let device = Device {
                id: id.into(),
                version: expected + 1,
                channel: "web_push".into(),
                owner: String::new(),
                token: token.into(),
                groups: vec![],
                enabled: true,
            };
            s.alert_device_register(&device, expected, owner, now)?;
            let version = old.as_ref().map_or(0, |d| d.version);
            let destination = Destination {
                id: id.into(),
                version: version + 1,
                channel: "web_push".into(),
                provider: provider.into(),
                target: format!("device:{id}"),
                enabled: true,
            };
            s.alert_destination_save(&destination, version, owner, now)?;
            Ok(json!({"device":{"id":id,"version":device.version,"enabled":true},"destination":id}))
        })
    }
    pub fn browser_push_status(&self, owner: &str, id: &str) -> Result<Value> {
        key(id)?;
        let d = self.alert_get::<Device>("device", id)?;
        if d.as_ref()
            .is_some_and(|d| d.owner != owner || d.channel != "web_push")
        {
            return Err("forbidden".into());
        }
        Ok(json!({"device":d.map(|d|json!({"id":d.id,"version":d.version,"enabled":d.enabled}))}))
    }
    pub fn browser_push_disable(&mut self, owner: &str, id: &str, now: i64) -> Result<Value> {
        self.browser_push_status(owner, id)?;
        if let Some(mut d) = self.alert_get::<Device>("device", id)? {
            if d.enabled {
                let expected = d.version;
                d.version += 1;
                d.enabled = false;
                self.alert_device_register(&d, expected, owner, now)?;
            }
        }
        self.browser_push_status(owner, id)
    }
    pub fn browser_push_alert(
        &self,
        owner: &str,
        id: &str,
        alert_key: &str,
        occurrence: u64,
        revision: u64,
    ) -> Result<Value> {
        self.browser_push_status(owner, id)?;
        let d = self
            .alert_get::<Device>("device", id)?
            .ok_or("device missing")?;
        if !d.enabled || !self.user_admin(owner).map_err(|_| "forbidden")? {
            return Err("forbidden".into());
        }
        let a = self.alert(alert_key)?;
        if a.occurrence != occurrence || a.revision != revision || a.acknowledgement.is_some() {
            return Err("notification superseded".into());
        }
        Ok(json!({"alert":a}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path as route};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    const ID: &str = "browser-11111111-1111-4111-8111-111111111111";
    async fn fixture_address(
        endpoint: &str,
    ) -> (String, std::path::PathBuf, ece::EcKeyComponents, [u8; 16]) {
        let path = std::env::temp_dir().join(format!(
            "talia-vapid-{}-{}.pem",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let output = std::process::Command::new("openssl")
            .args(["ecparam", "-name", "prime256v1", "-genkey", "-noout"])
            .output()
            .unwrap();
        assert!(output.status.success());
        std::fs::write(&path, output.stdout).unwrap();
        let public = URL_SAFE_NO_PAD.encode(
            signing_key(path.to_str().unwrap())
                .await
                .unwrap()
                .get_public_key(),
        );
        let (key, auth) = ece::generate_keypair_and_auth_secret().unwrap();
        let raw=json!({"subscription":{"endpoint":endpoint,"keys":{"p256dh":URL_SAFE_NO_PAD.encode(key.pub_as_raw().unwrap()),"auth":URL_SAFE_NO_PAD.encode(auth)}},"application_server_key":public}).to_string();
        (raw, path, key.raw_components().unwrap(), auth)
    }
    #[tokio::test]
    async fn encrypted_vapid_delivery_and_status_mapping() {
        let server = MockServer::start().await;
        let (raw, key, private, auth) =
            fixture_address(&(server.uri() + "/push?opaque=token")).await;
        let mut store = super::super::delivery::tests::fixture();
        store.alert_schedule(0).unwrap();
        let j = store.alert_deliveries().unwrap().remove(0);
        let mut d = store.alert_claim_delivery(&j.id, 0).unwrap().unwrap();
        d.address = raw;
        d.destination.channel = "web_push".into();
        d.delivery.device = Some(ID.into());
        d.delivery.expires = chrono::Utc::now().timestamp_millis() + 60000;
        for status in [201, 202, 404, 410, 429, 503, 302, 401] {
            server.reset().await;
            Mock::given(method("POST"))
                .and(route("/push"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .insert_header("retry-after", "2")
                        .insert_header("location", "http://127.0.0.1/private"),
                )
                .expect(1)
                .mount(&server)
                .await;
            let outcome = send(
                key.to_str().unwrap(),
                "mailto:operator@example.test",
                &[server.uri()],
                &d,
            )
            .await;
            match (status, outcome) {
                (201 | 202, Outcome::Sent)
                | (404 | 410, Outcome::ExpiredAddress(_))
                | (429 | 503, Outcome::RetryAfter(_, 2000))
                | (302 | 401, Outcome::Failed(_)) => (),
                other => panic!("unexpected {}", other.0),
            }
            let requests = server.received_requests().await.unwrap();
            let r = &requests[0];
            assert_eq!(r.headers["content-encoding"], "aes128gcm");
            assert!(r.headers["authorization"]
                .to_str()
                .unwrap()
                .starts_with("vapid t="));
            let payload: Value =
                serde_json::from_slice(&ece::decrypt(&private, &auth, &r.body).unwrap()).unwrap();
            assert_eq!(payload["device"], ID);
            assert_eq!(payload["key"], d.alert.key);
            assert!(payload.get("message").is_none());
            assert!(r.headers["ttl"].to_str().unwrap().parse::<u32>().unwrap() <= 60);
            server.verify().await;
        }
        assert!(matches!(
            send(
                key.to_str().unwrap(),
                "mailto:operator@example.test",
                &default_origins(),
                &d
            )
            .await,
            Outcome::Failed(_)
        ));
        std::fs::remove_file(key).unwrap();
    }
    #[tokio::test]
    async fn channel_isolation_and_expired_subscription_rotation() {
        let (raw, key, _, _) = fixture_address("https://fcm.googleapis.com/push/old").await;
        let mut s = super::super::delivery::tests::fixture();
        s.user_bootstrap("owner").unwrap();
        s.browser_push_register("owner", ID, &raw, "browser", 0)
            .unwrap();
        let destination = s
            .alert_destinations()
            .unwrap()
            .into_iter()
            .find(|d| d.id == ID)
            .unwrap();
        assert_eq!(
            s.destination_targets(&destination).unwrap(),
            vec![Some(ID.into())]
        );
        let mut android = destination.clone();
        android.channel = "push".into();
        assert!(s.destination_targets(&android).unwrap().is_empty());
        s.alert_schedule(0).unwrap();
        let jobs = s.alert_deliveries().unwrap();
        for (i, job) in jobs.iter().enumerate() {
            let mut dispatch = s.alert_claim_delivery(&job.id, 0).unwrap().unwrap();
            dispatch.delivery.device = Some(ID.into());
            s.alert_put("delivery", &job.id, &dispatch.delivery)
                .unwrap();
            let expired = if i == 0 {
                raw.replace("/old", "/rotated-away")
            } else {
                raw.clone()
            };
            s.alert_finish_delivery(&job.id, Outcome::ExpiredAddress(expired), 1)
                .unwrap();
            assert_eq!(
                s.browser_push_status("owner", ID).unwrap()["device"]["enabled"],
                i == 0
            );
            let stored = s.alert_deliveries().unwrap();
            assert!(!json!(stored).to_string().contains("fcm.googleapis.com"));
        }
        assert!(s.destination_targets(&destination).unwrap().is_empty());
        std::fs::remove_file(key).unwrap();
    }
    #[tokio::test]
    async fn browser_registration_ownership_disable_and_stale_alerts() {
        let (raw, key, _, _) = fixture_address("https://fcm.googleapis.com/push/secret").await;
        let mut s = super::super::delivery::tests::fixture();
        s.user_bootstrap("owner").unwrap();
        s.browser_push_register("owner", ID, &raw, "browser", 0)
            .unwrap();
        assert!(s
            .browser_push_register("other", ID, &raw, "browser", 0)
            .is_err());
        assert!(s.browser_push_disable("other", ID, 0).is_err());
        assert!(!json!(s.alert_devices_public().unwrap())
            .to_string()
            .contains("secret"));
        let mut cfg = s.alert_policies().unwrap().remove(0);
        let expected = cfg.version;
        cfg.version += 1;
        for stage in cfg.stages.values_mut() {
            for action in &mut stage.actions {
                action.destinations = vec![ID.into()];
            }
        }
        s.alert_policy_save(&cfg, expected, &BTreeMap::new(), "owner", 0)
            .unwrap();
        // Registration itself never sends or schedules an alert.
        assert!(s.alert_deliveries().unwrap().is_empty());
        let device = s.alert_get::<Device>("device", ID).unwrap().unwrap();
        assert_eq!(device.channel, "web_push");
        let alert = s.alerts().unwrap().remove(0);
        assert!(s
            .browser_push_alert("other", ID, &alert.key, alert.occurrence, alert.revision)
            .is_err());
        assert!(s
            .browser_push_alert(
                "owner",
                ID,
                &alert.key,
                alert.occurrence,
                alert.revision + 1
            )
            .is_err());
        assert!(s
            .browser_push_alert("owner", ID, &alert.key, alert.occurrence, alert.revision)
            .is_ok());
        s.browser_push_disable("owner", ID, 1).unwrap();
        assert_eq!(
            s.browser_push_status("owner", ID).unwrap()["device"]["enabled"],
            false
        );
        assert!(s
            .browser_push_alert("owner", ID, &alert.key, alert.occurrence, alert.revision)
            .is_err());
        std::fs::remove_file(key).unwrap();
    }
}
