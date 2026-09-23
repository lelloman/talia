use super::*;
use std::sync::{Arc, Mutex};
use wiremock::{
    matchers::{method, path, path_regex},
    Mock, MockServer, ResponseTemplate,
};
fn fixture() -> (Worker, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("talia-telegram-{}", random().unwrap()));
    std::fs::create_dir(&dir).unwrap();
    let mut s = Store::open(dir.join("engine.db")).unwrap();
    s.user_bootstrap("admin").unwrap();
    s.user_seen("viewer", "viewer").unwrap();
    (Worker::new(Engine::with_clock(s, Rc::new(|| 1000))), dir)
}
async fn bot_fixture() -> MockServer {
    let server = MockServer::start().await;
    transport::TEST_BASE.with(|v| *v.borrow_mut() = Some(server.uri()));
    Mock::given(path("/bot123:fixture/getMe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ok":true,"result":{"id":123,"is_bot":true,"username":"talia_fixture_bot"}}),
        ))
        .mount(&server)
        .await;
    Mock::given(path("/bot123:fixture/getWebhookInfo"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":{"url":""}})),
        )
        .mount(&server)
        .await;
    server
}
async fn connect(w: &Worker) {
    w.admin(
        "admin",
        json!({"op":"telegramConnect","token":"123:fixture","expected":0}),
    )
    .await
    .unwrap();
}
async fn pair(w: &Worker, chat: i64, user: i64, investigate: bool) {
    let code = w
        .admin("admin", json!({"op":"telegramPair"}))
        .await
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    w.engine.store.borrow_mut().telegram_ingest(&json!({"update_id":chat.abs(),"message":{"chat":{"id":chat,"type":if chat>0{"private"}else{"group"},"title":"Fixture"},"from":{"id":user,"is_bot":false,"username":"fixture"},"text":format!("/pair {code}")}}),1000).unwrap();
    w.admin(
        "admin",
        json!({"op":"telegramApprove","code":code,"delivery":true,"investigate":investigate}),
    )
    .await
    .unwrap();
}
#[tokio::test]
async fn pairing_encryption_authority_and_revocation() {
    let server = bot_fixture().await;
    let (w, dir) = fixture();
    connect(&w).await;
    assert!(w
        .admin("viewer", json!({"op":"telegramStatus"}))
        .await
        .is_err());
    let stored = w.engine.store.borrow().telegram_config().unwrap().token;
    assert!(!stored.contains("fixture"));
    let status = w
        .admin("admin", json!({"op":"telegramStatus"}))
        .await
        .unwrap();
    assert!(!status.to_string().contains("123:fixture"));
    assert_eq!(Bot::load(&w.engine).await.unwrap().1.token, "123:fixture");
    pair(&w, 41, 41, false).await;
    assert!(!w.engine.store.borrow().telegram_authorized(41, 41).unwrap());
    // Same username never grants access. Both the numeric account and chat must be approved.
    pair(&w, 42, 42, true).await;
    assert!(w.engine.store.borrow().telegram_authorized(42, 42).unwrap());
    assert!(!w.engine.store.borrow().telegram_authorized(41, 42).unwrap());
    assert!(!w.engine.store.borrow().telegram_authorized(42, 99).unwrap());
    let token = w
        .admin("admin", json!({"op":"telegramObserverKey"}))
        .await
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        observer::execute(&w.engine, &token, "observer_snapshot", json!({}))
            .await
            .is_ok()
    );
    for name in [
        "definitions_save",
        "engine_write",
        "reports_run",
        "alerts_acknowledge",
        "live_execute",
    ] {
        assert!(observer::execute(&w.engine, &token, name, json!({}))
            .await
            .is_err());
    }
    w.admin("admin", json!({"op":"telegramObserverRevoke"}))
        .await
        .unwrap();
    assert!(
        observer::execute(&w.engine, &token, "observer_snapshot", json!({}))
            .await
            .is_err()
    );
    w.admin("admin", json!({"op":"telegramRevokeUser","id":42}))
        .await
        .unwrap();
    assert!(!w.engine.store.borrow().telegram_authorized(42, 42).unwrap());
    drop(server);
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn delivery_chunks_are_tracked_and_never_replayed_after_uncertainty() {
    let server = bot_fixture().await;
    let (w, dir) = fixture();
    connect(&w).await;
    pair(&w, 10, 10, false).await;
    let bodies = Arc::new(Mutex::new(vec![]));
    let copy = bodies.clone();
    Mock::given(path("/bot123:fixture/sendMessage"))
        .respond_with(move |r: &wiremock::Request| {
            let v: Value = serde_json::from_slice(&r.body).unwrap();
            copy.lock().unwrap().push(v);
            ResponseTemplate::new(200).set_body_json(
                json!({"ok":true,"result":{"message_id":copy.lock().unwrap().len()}}),
            )
        })
        .mount(&server)
        .await;
    w.engine
        .store
        .borrow()
        .telegram_enqueue("report-a", 10, "report", Some("run-1"), &"😀".repeat(2000))
        .unwrap();
    let (c, bot) = Bot::load(&w.engine).await.unwrap();
    for _ in 0..3 {
        w.dispatch(&bot, c.version).await.unwrap();
    }
    assert_eq!(bodies.lock().unwrap().len(), 2);
    for v in bodies.lock().unwrap().iter() {
        assert!(v["text"].as_str().unwrap().encode_utf16().count() <= 4096);
        assert!(v.get("parse_mode").is_none());
    }
    assert_eq!(
        w.engine
            .store
            .borrow()
            .telegram_delivery_status("report-a")
            .unwrap(),
        "sent"
    );
    w.engine
        .store
        .borrow()
        .telegram_enqueue("lost", 10, "test", None, "unknown")
        .unwrap();
    w.engine
        .store
        .borrow()
        .conn
        .execute(
            "UPDATE telegram_outbox SET status='sending' WHERE id='lost-000'",
            [],
        )
        .unwrap();
    w.engine.store.borrow().telegram_recover().unwrap();
    w.dispatch(&bot, c.version).await.unwrap();
    assert_eq!(bodies.lock().unwrap().len(), 2);
    assert_eq!(
        w.engine
            .store
            .borrow()
            .telegram_delivery_status("lost")
            .unwrap(),
        "unknown"
    );
    // The report worker waits for actual Telegram acceptance, not just enqueue.
    let report = {
        let mut s = w.engine.store.borrow_mut();
        let d:crate::reports::Definition=serde_json::from_value(json!({"id":"telegram-report","version":1,"enabled":false,"steps":[{"id":"data","kind":"script","source":"()=>42"}],"compose":"()=>({subject:'Fixture',summary:'Report delivered',sections:[]})","destinations":["telegram-10"]})).unwrap();
        s.report_save(&d, 0, 1000).unwrap();
        s.report_start(&d.id, "admin", true, 1000).unwrap()
    };
    let reports = crate::reports::worker::Worker::new(w.engine.clone());
    for _ in 0..3 {
        reports.advance(&report.id).await.unwrap();
    }
    assert_eq!(
        w.engine
            .store
            .borrow()
            .report_run(&report.id)
            .unwrap()
            .status,
        "delivering"
    );
    w.dispatch(&bot, c.version).await.unwrap();
    reports.advance(&report.id).await.unwrap();
    reports.advance(&report.id).await.unwrap();
    assert_eq!(
        w.engine
            .store
            .borrow()
            .report_run(&report.id)
            .unwrap()
            .status,
        "complete"
    );
    assert_eq!(
        w.engine
            .store
            .borrow()
            .report_run(&report.id)
            .unwrap()
            .deliveries[0]
            .status,
        "sent"
    );
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn conversations_select_reports_compact_and_reconcile_agent_sessions() {
    let _bot = bot_fixture().await;
    let (w, dir) = fixture();
    connect(&w).await;
    pair(&w, 55, 55, true).await;
    let agents = MockServer::start().await;
    let request: Value = serde_json::from_str(include_str!(
        "../../vendor/simple-agents-client/contracts/v1/examples/observe.json"
    ))
    .unwrap();
    std::fs::write(dir.join("agent.token"), "fixture-secret").unwrap();
    let providers = dir.join("observers.json");
    std::fs::write(&providers,json!({"observer":{"origin":agents.uri(),"caller_id":"talia","token_file":dir.join("agent.token"),"profile_id":request["profile_id"],"capabilities":[],"budget":request["budget"]}}).to_string()).unwrap();
    observer::TEST_PROVIDERS.with(|v| *v.borrow_mut() = Some(providers.to_string_lossy().into()));
    w.admin("admin",json!({"op":"telegramSettings","expected":1,"enabled":true,"observer":"observer","sources":[]})).await.unwrap();
    let submissions = Arc::new(Mutex::new(
        std::collections::BTreeMap::<String, Value>::new(),
    ));
    let saved = submissions.clone();
    Mock::given(method("POST"))
        .and(path("/v1/sessions"))
        .respond_with(move |r: &wiremock::Request| {
            let v: Value = serde_json::from_slice(&r.body).unwrap();
            saved
                .lock()
                .unwrap()
                .insert(v["idempotency_key"].as_str().unwrap().into(), v);
            ResponseTemplate::new(503)
        })
        .mount(&agents)
        .await;
    let saved = submissions.clone();
    Mock::given(method("GET")).and(path_regex("/v1/sessions/by-key/.*")).respond_with(move|r:&wiremock::Request|{
        let key=r.url.path().rsplit('/').next().unwrap();let map=saved.lock().unwrap();
        if let Some(v)=map.get(key){ResponseTemplate::new(200).set_body_json(json!({"version":1,"session_id":key,"caller_id":"talia","source":v["source"],"profile_id":v["profile_id"],"state":"succeeded","resource_version":2,"attempt":1,"effective":{"profile_revision":1,"engine":"fixture","engine_version":"1","capabilities":[],"binding_revisions":[],"budget":v["budget"]},"created_at_ms":1000,"retain_until_ms":99999999}))}else{ResponseTemplate::new(404)}
    }).mount(&agents).await;
    Mock::given(method("GET")).and(path_regex("/v1/sessions/.*/result")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"outcome":"succeeded","summary":"Everything looks healthy. Report reference retained.","artifact_ids":[],"effect_receipts":[]}))).mount(&agents).await;
    // A delivered report does not automatically enter conversation history.
    w.engine
        .store
        .borrow()
        .telegram_enqueue(
            "daily",
            55,
            "report",
            Some("unrelated-report"),
            "DO NOT INCLUDE THIS REPORT",
        )
        .unwrap();
    let message = |id, text: &str| json!({"update_id":id,"message":{"chat":{"id":55,"type":"private"},"from":{"id":55,"is_bot":false},"text":text}});
    w.engine
        .store
        .borrow_mut()
        .telegram_ingest(&message(60, "What is the current status?"), 1000)
        .unwrap();
    w.engine
        .store
        .borrow_mut()
        .telegram_ingest(&message(60, "What is the current status?"), 1000)
        .unwrap();
    w.conversation().await.unwrap();
    w.conversation().await.unwrap();
    let s = Store::open(dir.join("engine.db")).unwrap();
    let resumed = Worker::new(Engine::with_clock(s, Rc::new(|| 7000)));
    resumed.conversation().await.unwrap();
    assert_eq!(submissions.lock().unwrap().len(), 1);
    let body = submissions
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .to_string();
    assert!(!body.contains("DO NOT INCLUDE"));
    assert!(!body.contains("unrelated-report"));
    resumed
        .engine
        .store
        .borrow_mut()
        .telegram_ingest(&message(61, "/compact"), 7000)
        .unwrap();
    resumed.conversation().await.unwrap();
    resumed.conversation().await.unwrap();
    let s = Store::open(dir.join("engine.db")).unwrap();
    let compacted = Worker::new(Engine::with_clock(s, Rc::new(|| 13000)));
    compacted.conversation().await.unwrap();
    let (summary, through): (String, i64) = compacted
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT summary,through FROM telegram_context", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert!(!summary.is_empty());
    assert!(through > 0);
    compacted
        .engine
        .store
        .borrow_mut()
        .telegram_ingest(&message(62, "/new"), 13000)
        .unwrap();
    let summary: String = compacted
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT summary FROM telegram_context", [], |r| r.get(0))
        .unwrap();
    assert_eq!(summary, "");
    let retained: i64 = compacted
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT count(*) FROM telegram_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(retained, 2);
    // Reply linkage uses our stored message ID, not quoted/forwarded Telegram text.
    let report_id = {
        let mut s = compacted.engine.store.borrow_mut();
        let d:crate::reports::Definition=serde_json::from_value(json!({"id":"chosen","version":1,"enabled":false,"steps":[{"id":"data","kind":"script","source":"()=>null"}],"compose":"()=>({subject:'Chosen',summary:'Selected evidence',sections:[]})","destinations":["telegram-55"]})).unwrap();
        s.report_save(&d, 0, 13000).unwrap();
        let mut r = s.report_start("chosen", "admin", false, 13000).unwrap();
        r.text = Some("SELECTED REPORT EVIDENCE".into());
        r.status = "complete".into();
        s.report_put(&r).unwrap();
        s.telegram_report("selected", 55, &r).unwrap();
        s.conn
            .execute(
                "UPDATE telegram_outbox SET status='sent',message_id=777 WHERE id='selected-000'",
                [],
            )
            .unwrap();
        r.id
    };
    let mut reply = message(63, "Explain this report");
    reply["message"]["reply_to_message"] = json!({"message_id":777,"text":"FORGED QUOTED CONTENT"});
    compacted
        .engine
        .store
        .borrow_mut()
        .telegram_ingest(&reply, 13000)
        .unwrap();
    compacted.conversation().await.unwrap();
    let pending: String = compacted
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT body FROM telegram_jobs WHERE id=63", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(pending.contains(&report_id));
    assert!(pending.contains("SELECTED REPORT EVIDENCE"));
    assert!(!pending.contains("FORGED QUOTED CONTENT"));
    assert!(!pending.contains("DO NOT INCLUDE"));
    compacted
        .engine
        .store
        .borrow_mut()
        .telegram_ingest(&message(64, "/new"), 13000)
        .unwrap();
    for _ in 0..16 {
        compacted.engine.store.borrow().conn.execute("INSERT INTO telegram_history(chat,user,epoch,role,body) VALUES(55,55,2,'user','older conversation')",[]).unwrap();
    }
    compacted
        .engine
        .store
        .borrow_mut()
        .telegram_ingest(&message(65, "Investigate again"), 13000)
        .unwrap();
    compacted.conversation().await.unwrap();
    let pending: String = compacted
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT body FROM telegram_jobs WHERE id=65", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&pending).unwrap()["phase"],
        "auto_compact"
    );
    compacted
        .admin("admin", json!({"op":"telegramRevokeUser","id":55}))
        .await
        .unwrap();
    let status: String = compacted
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT status FROM telegram_jobs WHERE id=65", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status, "cancelled");
    drop(compacted);
    drop(resumed);
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn diagnostic_probes_require_source_approval_and_reject_writes() {
    let server = MockServer::start().await;
    let (w, dir) = fixture();
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"healthy":true})))
        .expect(1)
        .mount(&server)
        .await;
    {
        let mut s = w.engine.store.borrow_mut();
        let mut monitoring = s.monitoring_config().unwrap();
        let expected = monitoring.version;
        monitoring.version += 1;
        monitoring.sources.push(crate::monitoring::DataSource {
            id: "health".into(),
            kind: "http".into(),
            url: server.uri(),
            credential_ref: None,
            timeout_ms: 1000,
            max_bytes: 4096,
        });
        s.configure_monitoring(&monitoring, expected).unwrap();
    }
    let token = w
        .admin("admin", json!({"op":"telegramObserverKey"}))
        .await
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let args = json!({"source":"health","request":{"kind":"http","path":"/health","method":"GET"}});
    assert!(
        observer::execute(&w.engine, &token, "observer_probe", args.clone())
            .await
            .is_err()
    );
    {
        let s = w.engine.store.borrow();
        let mut c = s.telegram_config().unwrap();
        c.sources = vec!["health".into()];
        s.telegram_put(&c).unwrap();
    }
    assert!(observer::execute(&w.engine, &token, "observer_probe", args)
        .await
        .is_ok());
    assert!(observer::execute(
        &w.engine,
        &token,
        "observer_probe",
        json!({"source":"health","request":{"kind":"http","path":"/health","method":"POST"}})
    )
    .await
    .is_err());
    server.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
}
