use super::*;
use std::sync::{Arc, Mutex};
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
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
    assert!(w
        .admin(
            "admin",
            json!({"op":"telegramSettings","expected":1,"enabled":true,"observer":"legacy"})
        )
        .await
        .is_err());
    w.admin(
        "admin",
        json!({"op":"telegramSettings","expected":1,"enabled":false}),
    )
    .await
    .unwrap();
    assert!(!w.engine.store.borrow().telegram_config().unwrap().enabled);
    w.admin(
        "admin",
        json!({"op":"telegramSettings","expected":2,"enabled":true}),
    )
    .await
    .unwrap();
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
    pair(&w, 42, 42, false).await;
    // Existing numeric permissions survive an upgrade.
    {
        let store = w.engine.store.borrow();
        store.conn.execute("UPDATE telegram_peers SET body=json_set(body,'$.investigate',json('true')) WHERE chat=42",[]).unwrap();
        store
            .conn
            .execute("INSERT INTO telegram_users VALUES(42,'fixture')", [])
            .unwrap();
    }
    w.engine.store.borrow_mut().telegram_ingest(&json!({"update_id":100,"message":{"chat":{"id":42,"type":"private"},"from":{"id":42,"is_bot":false},"text":"/ask check my server"}}),1000).unwrap();
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row("SELECT count(*) FROM telegram_jobs", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(w
        .engine
        .store
        .borrow()
        .conn
        .query_row(
            "SELECT body FROM telegram_outbox WHERE id='disabled-100-000'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap()
        .contains("not configured"));
    assert!(w.engine.store.borrow().telegram_authorized(42, 42).unwrap());
    assert!(!w.engine.store.borrow().telegram_authorized(41, 42).unwrap());
    assert!(!w.engine.store.borrow().telegram_authorized(42, 99).unwrap());
    assert!(w
        .admin("admin", json!({"op":"telegramObserverKey"}))
        .await
        .is_err());
    assert_eq!(
        w.admin("admin", json!({"op":"telegramStatus"}))
            .await
            .unwrap()["investigationsAvailable"],
        false
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
async fn conversations_select_reports_compact_and_start_fresh() {
    let _bot = bot_fixture().await;
    let (w, dir) = fixture();
    connect(&w).await;
    pair(&w, 55, 55, true).await;
    let ai = MockServer::start().await;
    let ai_dir = crate::ai::tests::config(&ai.uri());
    w.admin("admin",json!({"op":"telegramSettings","expected":1,"enabled":true,"investigations":true,"sources":[]})).await.unwrap();
    let contexts = Arc::new(Mutex::new(vec![]));
    let copy = contexts.clone();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            let compact = request["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("Compact this conversation");
            copy.lock().unwrap().push(request);
            ResponseTemplate::new(200).set_body_json(crate::ai::tests::answer(if compact {
                "Concise prior conversation"
            } else {
                "Monitoring answer"
            }))
        })
        .mount(&ai)
        .await;
    let mut run_ids = vec![];
    {
        let mut s = w.engine.store.borrow_mut();
        for (id, text) in [
            ("selected", "SELECTED-REPORT"),
            ("other", "UNRELATED-REPORT"),
        ] {
            let d:crate::reports::Definition=serde_json::from_value(json!({"id":id,"version":1,"enabled":false,"steps":[{"id":"data","kind":"script","source":"()=>42"}],"compose":"()=>({subject:'Fixture',summary:'Report',sections:[]})","destinations":[]})).unwrap();
            s.report_save(&d, 0, 1000).unwrap();
            let mut r = s.report_start(id, "admin", false, 1000).unwrap();
            r.status = "complete".into();
            r.text = Some(text.into());
            s.report_put(&r).unwrap();
            run_ids.push(r.id);
        }
        s.conn.execute("INSERT INTO telegram_outbox(id,chat,kind,reference,status,body,message_id) VALUES('selected',55,'report',?,'sent','report',777)",[&run_ids[0]]).unwrap();
    }
    let ingest = |id, text: &str, reply: bool| {
        let mut m =
            json!({"chat":{"id":55,"type":"private"},"from":{"id":55,"is_bot":false},"text":text});
        if reply {
            m["reply_to_message"] = json!({"message_id":777,"text":"FORGED-QUOTED-REPORT"});
        }
        w.engine
            .store
            .borrow_mut()
            .telegram_ingest(&json!({"update_id":id,"message":m}), 1000)
            .unwrap();
    };
    ingest(60, "What is happening?", false);
    w.conversation().await.unwrap();
    ingest(61, "Explain this report", true);
    w.conversation().await.unwrap();
    ingest(62, "/compact", false);
    w.conversation().await.unwrap();
    {
        let q = contexts.lock().unwrap();
        let first: Value =
            serde_json::from_str(q[0]["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert!(first["referenced_report"].is_null());
        assert!(q[1].to_string().contains("SELECTED-REPORT"));
        assert!(!q.iter().any(|v| v.to_string().contains("UNRELATED-REPORT")
            || v.to_string().contains("FORGED-QUOTED-REPORT")));
        assert!(q[2].get("tools").is_none());
        assert!(!q[2].to_string().contains("SELECTED-REPORT"));
    }
    let history: i64 = w
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT count(*) FROM telegram_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(history, 4);
    ingest(63, "/new", false);
    ingest(64, "Fresh question", false);
    w.conversation().await.unwrap();
    let fresh: Value = serde_json::from_str(
        contexts.lock().unwrap()[3]["messages"][1]["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(fresh["summary"], "");
    assert_eq!(fresh["messages"], json!([]));
    {
        let s = w.engine.store.borrow();
        for i in 0..16 {
            s.conn.execute("INSERT INTO telegram_history(chat,user,epoch,role,body) VALUES(55,55,1,'user',?)",[format!("observation {i}")]).unwrap();
        }
    }
    ingest(65, "Check again", false);
    w.conversation().await.unwrap();
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row("SELECT status FROM telegram_jobs WHERE id=65", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "queued"
    );
    w.conversation().await.unwrap();
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row("SELECT status FROM telegram_jobs WHERE id=65", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "done"
    );
    assert!(contexts.lock().unwrap()[4].get("tools").is_none());
    assert!(contexts.lock().unwrap()[5].get("tools").is_some());
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn inference_does_not_block_bot_delivery_and_revocation_suppresses_answer() {
    let bot_server = bot_fixture().await;
    let (w, dir) = fixture();
    connect(&w).await;
    pair(&w, 55, 55, true).await;
    let ai = MockServer::start().await;
    let ai_dir = crate::ai::tests::config(&ai.uri());
    w.admin("admin",json!({"op":"telegramSettings","expected":1,"enabled":true,"investigations":true,"sources":[]})).await.unwrap();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_json(crate::ai::tests::answer("LATE-ANSWER")),
        )
        .expect(1)
        .mount(&ai)
        .await;
    Mock::given(path("/bot123:fixture/getUpdates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":[]})))
        .expect(1)
        .mount(&bot_server)
        .await;
    Mock::given(path("/bot123:fixture/sendMessage"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":{"message_id":1}})),
        )
        .expect(1)
        .mount(&bot_server)
        .await;
    w.engine.store.borrow_mut().telegram_ingest(&json!({"update_id":60,"message":{"chat":{"id":55,"type":"private"},"from":{"id":55,"is_bot":false},"text":"Investigate"}}),1000).unwrap();
    w.admin("admin", json!({"op":"telegramTest","chat":55}))
        .await
        .unwrap();
    let finished = Cell::new(false);
    let (conversation, _) = tokio::join!(
        async {
            let r = w.conversation().await;
            finished.set(true);
            r
        },
        async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            w.advance().await.unwrap();
            assert!(!finished.get());
            w.admin("admin", json!({"op":"telegramRevokeUser","id":55}))
                .await
                .unwrap();
        }
    );
    conversation.unwrap();
    let s = w.engine.store.borrow();
    assert_eq!(
        s.conn
            .query_row("SELECT status FROM telegram_jobs WHERE id=60", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "cancelled"
    );
    assert_eq!(
        s.conn
            .query_row(
                "SELECT count(*) FROM telegram_outbox WHERE body LIKE '%LATE-ANSWER%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(s);
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}
