use super::account::*;
use super::*;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};
struct Fixture {
    e: Engine,
    now: Rc<Cell<i64>>,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "talia-account-{}",
            crate::telegram::transport::random().unwrap()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut s = Store::open(dir.join("engine.db")).unwrap();
        s.user_bootstrap("admin").unwrap();
        s.user_seen("viewer", "Viewer").unwrap();
        let now = Rc::new(Cell::new(1_000_000));
        let c = now.clone();
        Self {
            e: Engine::with_clock(s, Rc::new(move || c.get())),
            now,
            dir,
        }
    }
    async fn op(&self, op: &str) -> Value {
        admin(&self.e, "admin", json!({"op":op,"expected":1}))
            .await
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
async fn provider() -> MockServer {
    let s = MockServer::start().await;
    let o = s.uri();
    for (p, body) in [
        (
            "/.well-known/simple-ai",
            json!({"issuer":o,"client_id":"public-ai"}),
        ),
        (
            "/.well-known/openid-configuration",
            json!({"issuer":o,"device_authorization_endpoint":format!("{o}/device_authorization"),"token_endpoint":format!("{o}/token"),"userinfo_endpoint":format!("{o}/userinfo")}),
        ),
        (
            "/device_authorization",
            json!({"device_code":"secret-device","user_code":"PUBLIC-CODE","verification_uri":format!("{o}/device"),"expires_in":600,"interval":5}),
        ),
        (
            "/userinfo",
            json!({"sub":"dedicated-talia","email":"talia@example.com","name":"Talìa"}),
        ),
    ] {
        Mock::given(path(p))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&s)
            .await;
    }
    s
}
fn tokens(access: &str, refresh: &str) -> Value {
    json!({"access_token":access,"refresh_token":refresh,"expires_in":60,"token_type":"Bearer"})
}
async fn begin(f: &Fixture, s: &MockServer) -> Value {
    admin(
        &f.e,
        "admin",
        json!({"op":"aiAccountBegin","expected":0,"origin":s.uri(),"model":"class:fast"}),
    )
    .await
    .unwrap()
}
async fn connected(f: &Fixture, s: &MockServer) {
    Mock::given(path("/token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(tokens("secret-access", "secret-refresh")),
        )
        .mount(s)
        .await;
    begin(f, s).await;
    f.now.set(f.now.get() + 5000);
    assert_eq!(f.op("aiAccountPoll").await["phase"], "confirm");
    assert_eq!(f.op("aiAccountConfirm").await["phase"], "connected");
}
#[tokio::test]
async fn connect_confirm_encrypt_restart_disconnect_no_fallback() {
    let s = provider().await;
    let f = Fixture::new();
    let pending = begin(&f, &s).await;
    assert_eq!(pending["pending"]["code"], "PUBLIC-CODE");
    assert!(!pending.to_string().contains("secret-device"));
    assert!(session(&f.e).await.is_err());
    assert!(admin(&f.e, "viewer", json!({"op":"aiAccountStatus"}))
        .await
        .is_err());
    assert!(admin(
        &f.e,
        "viewer",
        json!({"op":"aiAccountDisconnect","expected":1})
    )
    .await
    .is_err());
    assert!(
        admin(&f.e, "admin", json!({"op":"aiAccountConfirm","expected":1}))
            .await
            .is_err()
    );
    assert_eq!(f.op("aiAccountPoll").await["phase"], "pending"); // server enforces interval
    Mock::given(path("/token"))
        .and(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(tokens("secret-access", "secret-refresh")),
        )
        .expect(1)
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 5000);
    let identity = f.op("aiAccountPoll").await;
    assert_eq!(identity["identity"]["sub"], "dedicated-talia");
    assert!(session(&f.e).await.is_err());
    f.op("aiAccountConfirm").await;
    assert_eq!(session(&f.e).await.unwrap().token, "secret-access");
    let db = std::fs::read(f.dir.join("engine.db")).unwrap();
    for secret in ["secret-access", "secret-refresh", "secret-device"] {
        assert!(!String::from_utf8_lossy(&db).contains(secret));
        assert!(!identity.to_string().contains(secret));
    }
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(f.dir.join("engine.db.ai-key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let clock = f.now.clone();
    let reopened = Engine::with_clock(
        Store::open(f.dir.join("engine.db")).unwrap(),
        Rc::new(move || clock.get()),
    );
    reopened.store.borrow().ai_account_recover().unwrap();
    assert_eq!(session(&reopened).await.unwrap().token, "secret-access");
    f.op("aiAccountDisconnect").await;
    assert!(session(&f.e).await.is_err());
    assert!(check(&f.e, Some(1)).is_err());
    assert!(account::configuration(&f.e).await.is_err());
    assert!(
        admin(&f.e, "admin", json!({"op":"aiAccountConfirm","expected":1}))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn refresh_is_single_flight_rotated_and_persisted() {
    let s = provider().await;
    let f = Fixture::new();
    connected(&f, &s).await;
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    Mock::given(path("/token"))
        .respond_with(move |r: &wiremock::Request| {
            c.fetch_add(1, Ordering::SeqCst);
            let form = String::from_utf8_lossy(&r.body);
            assert!(form.contains("refresh_token=secret-refresh"));
            ResponseTemplate::new(200)
                .set_body_json(tokens("rotated-access", "rotated-refresh"))
                .set_delay(Duration::from_millis(80))
        })
        .with_priority(1)
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 40_000);
    let (a, b) = tokio::join!(session(&f.e), session(&f.e));
    assert_eq!(a.unwrap().token, "rotated-access");
    assert_eq!(b.unwrap().token, "rotated-access");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let clock = f.now.clone();
    let reopened = Engine::with_clock(
        Store::open(f.dir.join("engine.db")).unwrap(),
        Rc::new(move || clock.get()),
    );
    assert_eq!(session(&reopened).await.unwrap().token, "rotated-access");
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn interrupted_refresh_never_replays_old_token_and_identity_change_fails() {
    let s = provider().await;
    let f = Fixture::new();
    connected(&f, &s).await;
    Mock::given(path("/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(tokens("new-access", "new-refresh"))
                .set_delay(Duration::from_millis(200)),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 40_000);
    assert!(
        tokio::time::timeout(Duration::from_millis(60), session(&f.e))
            .await
            .is_err()
    );
    assert!(session(&f.e).await.is_err());
    f.e.store.borrow().ai_account_recover().unwrap();
    assert_eq!(
        admin(&f.e, "admin", json!({"op":"aiAccountStatus"}))
            .await
            .unwrap()["phase"],
        "reconnect"
    );
    let s = provider().await;
    let f = Fixture::new();
    connected(&f, &s).await;
    Mock::given(path("/userinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"sub":"wrong-account"})))
        .with_priority(1)
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 40_000);
    assert!(session(&f.e).await.is_err());
    assert_eq!(
        admin(&f.e, "admin", json!({"op":"aiAccountStatus"}))
            .await
            .unwrap()["phase"],
        "reconnect"
    );
}
#[tokio::test]
async fn revoked_refresh_and_disconnect_during_io_cannot_resurrect_account() {
    let s = provider().await;
    let f = Fixture::new();
    connected(&f, &s).await;
    Mock::given(path("/token"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(
                json!({"error":"invalid_grant","error_description":"PRIVATE UPSTREAM"}),
            ),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 40_000);
    let error = session(&f.e).await.err().unwrap();
    assert!(error.contains("revoked"));
    assert!(!error.contains("PRIVATE"));
    let s = provider().await;
    let f = Fixture::new();
    connected(&f, &s).await;
    Mock::given(path("/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(tokens("new-access", "new-refresh"))
                .set_delay(Duration::from_millis(100)),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 40_000);
    let (result, _) = tokio::join!(session(&f.e), async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        f.op("aiAccountDisconnect").await;
    });
    assert!(result.is_err());
    assert_eq!(
        admin(&f.e, "admin", json!({"op":"aiAccountStatus"}))
            .await
            .unwrap()["phase"],
        "disconnected"
    );
}
#[tokio::test]
async fn device_pending_slow_down_denial_expiry_and_owner_binding() {
    let s = provider().await;
    let f = Fixture::new();
    begin(&f, &s).await;
    Mock::given(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error":"slow_down"})))
        .expect(1)
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 5000);
    let v = f.op("aiAccountPoll").await;
    assert_eq!(v["pending"]["interval"], 10);
    f.op("aiAccountPoll").await; // no second provider call
    f.e.store
        .borrow_mut()
        .user_bootstrap("other-admin")
        .unwrap();
    // Bootstrap is additive; verify another administrator cannot confirm this flow.
    assert!(admin(
        &f.e,
        "other-admin",
        json!({"op":"aiAccountPoll","expected":1})
    )
    .await
    .is_err());
    let hidden = admin(&f.e, "other-admin", json!({"op":"aiAccountStatus"}))
        .await
        .unwrap();
    assert!(hidden["pending"].is_null());
    f.now.set(f.now.get() + 600_000);
    assert_eq!(f.op("aiAccountPoll").await["phase"], "reconnect");
    let s = provider().await;
    let f = Fixture::new();
    begin(&f, &s).await;
    Mock::given(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error":"access_denied"})))
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 5000);
    assert_eq!(f.op("aiAccountPoll").await["phase"], "reconnect");
}
#[tokio::test]
async fn rejects_unsafe_discovery_and_provider_redirects() {
    let s = provider().await;
    let f = Fixture::new();
    for origin in [
        "http://example.com",
        "https://user:password@example.com",
        "https://example.com/path",
        "https://example.com/?secret=1",
    ] {
        assert!(admin(
            &f.e,
            "admin",
            json!({"op":"aiAccountBegin","expected":0,"origin":origin,"model":"m"})
        )
        .await
        .is_err());
    }
    Mock::given(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"issuer":s.uri(),"device_authorization_endpoint":"https://evil.example/device"}),
        ))
        .with_priority(1)
        .mount(&s)
        .await;
    assert!(admin(
        &f.e,
        "admin",
        json!({"op":"aiAccountBegin","expected":0,"origin":s.uri(),"model":"m"})
    )
    .await
    .unwrap_err()
    .contains("issuer"));
    Mock::given(path("/.well-known/simple-ai"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "https://example.com")
                .set_body_json(json!({})),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    assert!(admin(
        &f.e,
        "admin",
        json!({"op":"aiAccountBegin","expected":0,"origin":s.uri(),"model":"m"})
    )
    .await
    .is_err());
}
#[tokio::test]
async fn userinfo_network_failure_keeps_rotated_credentials_for_safe_retry() {
    let s = provider().await;
    let f = Fixture::new();
    connected(&f, &s).await;
    Mock::given(path("/token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(tokens("rotated-access", "rotated-refresh")),
        )
        .expect(1)
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(path("/userinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_string("broken"))
        .with_priority(1)
        .up_to_n_times(1)
        .mount(&s)
        .await;
    f.now.set(f.now.get() + 40_000);
    assert!(session(&f.e).await.is_err());
    assert_eq!(session(&f.e).await.unwrap().token, "rotated-access");
}
#[tokio::test]
async fn managed_token_is_used_for_inference_and_disconnect_discards_delayed_result() {
    let s = provider().await;
    let f = Fixture::new();
    connected(&f, &s).await;
    Mock::given(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer secret-access"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(super::tests::answer("private analysis"))
                .set_delay(Duration::from_millis(100)),
        )
        .expect(1)
        .mount(&s)
        .await;
    // Use the same durable Telegram owner shape as the harness fixtures.
    f.e.store.borrow().conn.execute("INSERT INTO telegram_config VALUES(1,?)",[json!({"version":1,"enabled":true,"investigations":true,"sources":[],"bot_id":1,"username":"fixture","token":"sealed","offset":0,"last_poll":null,"error":null}).to_string()]).unwrap();
    f.e.store.borrow().conn.execute("INSERT INTO telegram_peers VALUES(1,?)",[json!({"chat":1,"name":"fixture","kind":"private","delivery":true,"investigate":true}).to_string()]).unwrap();
    f.e.store.borrow().conn.execute_batch("INSERT INTO telegram_users VALUES(1,'fixture');INSERT INTO telegram_context(chat,user) VALUES(1,1);INSERT INTO telegram_jobs VALUES(1,1,1,'queued','{}');").unwrap();
    let (result, _) = tokio::join!(
        execute(
            &f.e,
            "account-inference",
            Scope::Telegram {
                job: 1,
                chat: 1,
                user: 1,
                epoch: 0,
                revision: 1
            },
            f.now.get() + 100_000,
            "Summarize",
            json!({}),
            false
        ),
        async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            f.op("aiAccountDisconnect").await;
        }
    );
    assert!(result.is_err());
    assert!(!f
        .e
        .store
        .borrow()
        .ai_run("account-inference")
        .unwrap()
        .unwrap()
        .summary
        .unwrap_or_default()
        .contains("private analysis"));
}

#[tokio::test]
async fn disconnect_revokes_provider_session_and_survives_revocation_failure() {
    for succeeds in [true, false] {
        let server = provider().await;
        let f = Fixture::new();
        let origin = server.uri();
        Mock::given(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer":origin,"device_authorization_endpoint":format!("{origin}/device_authorization"),
                "token_endpoint":format!("{origin}/token"),"userinfo_endpoint":format!("{origin}/userinfo"),
                "revocation_endpoint":format!("{origin}/revoke")
            }))).with_priority(1).mount(&server).await;
        connected(&f, &server).await;
        Mock::given(path("/revoke"))
            .and(method("POST"))
            .respond_with(move |r: &wiremock::Request| {
                let form = String::from_utf8_lossy(&r.body);
                assert!(form.contains("token=secret-refresh"));
                assert!(form.contains("client_id=public-ai"));
                ResponseTemplate::new(if succeeds { 200 } else { 503 })
            })
            .expect(1)
            .mount(&server)
            .await;
        let legacy = super::tests::config(&origin);
        let result = f.op("aiAccountDisconnect").await;
        assert_eq!(result["phase"], "disconnected");
        assert_eq!(result["error"].is_null(), succeeds);
        assert!(session(&f.e).await.is_err());
        assert!(account::configuration(&f.e).await.is_err());
        std::fs::remove_dir_all(legacy).unwrap();
    }
}
