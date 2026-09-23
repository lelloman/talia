use super::*;
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};
pub(crate) fn config(origin: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "talia-ai-{}",
        crate::telegram::transport::random().unwrap()
    ));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("key"), "sk-fixture-secret").unwrap();
    std::fs::write(
        dir.join("config.json"),
        json!({"origin":origin,"model":"fixture-model","token_file":dir.join("key")}).to_string(),
    )
    .unwrap();
    TEST_CONFIG.with(|c| *c.borrow_mut() = Some(dir.join("config.json").to_string_lossy().into()));
    dir
}
pub(crate) fn answer(text: &str) -> Value {
    json!({"model":"fixture-model","choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}})
}
fn call(name: &str, args: Value) -> Value {
    json!({"choices":[{"message":{"role":"assistant","tool_calls":[{"id":"call-1","type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":"tool_calls"}]})
}
fn telegram(s: &Store) -> Scope {
    s.conn.execute("INSERT INTO telegram_config VALUES(1,?)",[json!({"version":1,"enabled":true,"investigations":true,"sources":["health"],"bot_id":1,"username":"fixture","token":"sealed","offset":0,"last_poll":null,"error":null}).to_string()]).unwrap();
    s.conn.execute("INSERT INTO telegram_peers VALUES(1,?)",[json!({"chat":1,"name":"fixture","kind":"private","delivery":true,"investigate":true}).to_string()]).unwrap();
    s.conn.execute_batch("INSERT INTO telegram_users VALUES(1,'fixture');INSERT INTO telegram_context(chat,user) VALUES(1,1);INSERT INTO telegram_jobs VALUES(1,1,1,'queued','{}');").unwrap();
    Scope::Telegram {
        job: 1,
        chat: 1,
        user: 1,
        epoch: 0,
        revision: 1,
    }
}
fn source(s: &mut Store, origin: &str) {
    let mut c = s.monitoring_config().unwrap();
    let expected = c.version;
    c.version += 1;
    c.sources.push(crate::monitoring::DataSource {
        id: "health".into(),
        kind: "http".into(),
        url: origin.into(),
        credential_ref: None,
        timeout_ms: 1000,
        max_bytes: 4096,
    });
    s.configure_monitoring(&c, expected).unwrap();
}
#[tokio::test]
async fn report_analysis_uses_selected_inputs_and_reuses_completed_run_after_reopen() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    let received = Arc::new(Mutex::new(None));
    let copy = received.clone();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-fixture-secret"))
        .respond_with(move |r: &wiremock::Request| {
            *copy.lock().unwrap() = Some(serde_json::from_slice::<Value>(&r.body).unwrap());
            ResponseTemplate::new(200).set_body_json(answer("All observed services are healthy"))
        })
        .expect(1)
        .mount(&http)
        .await;
    let mut s = Store::open(dir.join("engine.db")).unwrap();
    let d:crate::reports::Definition=serde_json::from_value(json!({"id":"morning","version":1,"enabled":false,"steps":[{"id":"private","kind":"script","source":"()=> 'unselected-marker'"},{"id":"facts","kind":"script","source":"()=> ({up:true})"},{"id":"analysis","kind":"analysis","instructions":"Summarize the facts","inputs":["facts"]}],"compose":"ctx=>({subject:'Morning',summary:ctx.steps.analysis.value.summary,sections:[]})","destinations":[]})).unwrap();
    s.report_save(&d, 0, 1000).unwrap();
    let r = s.report_start("morning", "admin", false, 1000).unwrap();
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let w = crate::reports::worker::Worker::new(e.clone());
    for _ in 0..3 {
        w.advance(&r.id).await.unwrap();
    }
    let request = received.lock().unwrap().clone().unwrap();
    assert!(request.get("tools").is_none());
    assert_eq!(request["stream"], false);
    assert!(!request.to_string().contains("unselected-marker"));
    assert!(request.to_string().contains("up"));
    // Simulate restart after inference committed but before its report step committed.
    let mut saved = e.store.borrow().report_run(&r.id).unwrap();
    saved.index = 2;
    saved.outputs.remove("analysis");
    e.store.borrow().report_put(&saved).unwrap();
    drop(w);
    drop(e);
    let s = Store::open(dir.join("engine.db")).unwrap();
    s.ai_recover().unwrap();
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let w = crate::reports::worker::Worker::new(e.clone());
    w.advance(&r.id).await.unwrap();
    w.advance(&r.id).await.unwrap();
    let done = e.store.borrow().report_run(&r.id).unwrap();
    assert_eq!(done.status, "complete");
    assert_eq!(
        done.content.unwrap().summary,
        "All observed services are healthy"
    );
    let ai = e
        .store
        .borrow()
        .ai_run(&format!("{}-analysis", r.id))
        .unwrap()
        .unwrap();
    assert_eq!(ai.turns, 1);
    assert!(!serde_json::to_string(&ai).unwrap().contains("sk-fixture"));
    http.verify().await;
    drop(w);
    drop(e);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn tool_loop_reads_and_probes_without_editing_authority() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    let reads = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"healthy":true})))
        .expect(1)
        .mount(&reads)
        .await;
    let requests = Arc::new(Mutex::new(vec![]));
    let copy = requests.clone();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let mut q = copy.lock().unwrap();
            q.push(serde_json::from_slice::<Value>(&r.body).unwrap());
            ResponseTemplate::new(200).set_body_json(match q.len() {
                1 => call("monitoring_snapshot", json!({})),
                2 => call(
                    "monitoring_probe",
                    json!({"source":"health","request":{"kind":"http","path":"/health"}}),
                ),
                _ => answer("The probe is healthy"),
            })
        })
        .expect(3)
        .mount(&http)
        .await;
    let mut s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    source(&mut s, &reads.uri());
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let v = execute(
        &e,
        "chat-1",
        scope,
        900000,
        "Check the service",
        json!({"question":"health?"}),
        true,
    )
    .await
    .unwrap();
    assert_eq!(v["summary"], "The probe is healthy");
    let q = requests.lock().unwrap();
    assert_eq!(q[0]["tools"].as_array().unwrap().len(), 3);
    assert_eq!(q[1]["messages"][3]["tool_call_id"], "call-1");
    assert!(q[2]["messages"].to_string().contains("healthy"));
    assert!(tools::execute(
        &e,
        "monitoring_probe",
        json!({"source":"health","request":{"kind":"http","method":"POST","path":"/health"}})
    )
    .await
    .is_err());
    assert!(tools::execute(
        &e,
        "monitoring_probe",
        json!({"source":"unapproved","request":{"kind":"http"}})
    )
    .await
    .is_err());
    drop(q);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn malformed_truncated_and_forbidden_completions_fail_without_retry() {
    for response in [
        call("engine_write", json!({"id":"anything","value":1})),
        json!({"choices":[{"message":{"role":"assistant","content":"unfinished"},"finish_reason":"length"}]}),
        json!({"choices":[]}),
    ] {
        let http = MockServer::start().await;
        let dir = config(&http.uri());
        Mock::given(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&http)
            .await;
        let s = Store::open(":memory:").unwrap();
        let scope = telegram(&s);
        let e = Engine::with_clock(s, Rc::new(|| 1000));
        for _ in 0..2 {
            assert!(execute(
                &e,
                "bad",
                scope.clone(),
                900000,
                "Investigate",
                json!({}),
                true
            )
            .await
            .is_err());
        }
        assert_eq!(
            e.store.borrow().ai_run("bad").unwrap().unwrap().status,
            "failed"
        );
        http.verify().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[tokio::test]
async fn turns_are_bounded_and_inflight_recovery_does_not_resubmit() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    Mock::given(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(call("monitoring_snapshot", json!({}))),
        )
        .expect(6)
        .mount(&http)
        .await;
    let s = Store::open(dir.join("engine.db")).unwrap();
    let scope = telegram(&s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    assert!(execute(
        &e,
        "loop",
        scope.clone(),
        900000,
        "Investigate",
        json!({}),
        true
    )
    .await
    .unwrap_err()
    .contains("turn limit"));
    let mut interrupted = e.store.borrow().ai_run("loop").unwrap().unwrap();
    interrupted.id = "interrupted".into();
    interrupted.status = "running".into();
    interrupted.error = None;
    e.store.borrow().ai_put(&interrupted).unwrap();
    drop(e);
    let s = Store::open(dir.join("engine.db")).unwrap();
    s.ai_recover().unwrap();
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    assert!(execute(
        &e,
        "interrupted",
        scope,
        900000,
        "Investigate",
        json!({}),
        true
    )
    .await
    .unwrap_err()
    .contains("restart"));
    http.verify().await;
    drop(e);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn revocation_during_completion_discards_the_answer() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    Mock::given(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(100))
                .set_body_json(answer("sensitive delayed answer")),
        )
        .expect(1)
        .mount(&http)
        .await;
    let s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let (result, _) = tokio::join!(
        execute(&e, "revoked", scope, 900000, "Investigate", json!({}), true),
        async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            e.store
                .borrow()
                .conn
                .execute("DELETE FROM telegram_users", [])
                .unwrap();
        }
    );
    assert!(result.unwrap_err().contains("permission"));
    let r = e.store.borrow().ai_run("revoked").unwrap().unwrap();
    assert!(r.summary.is_none());
    assert!(!serde_json::to_string(&r.messages)
        .unwrap()
        .contains("sensitive delayed answer"));
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn unsafe_connection_and_http_errors_do_not_expose_credentials() {
    let dir = config("http://example.com");
    assert!(configuration().await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    Mock::given(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "http://127.0.0.1:1/stolen")
                .set_body_string("sk-fixture-secret private error"),
        )
        .expect(1)
        .mount(&http)
        .await;
    let s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let error = execute(
        &e,
        "redirect",
        scope,
        900000,
        "Investigate",
        json!({}),
        true,
    )
    .await
    .unwrap_err();
    assert!(error.contains("HTTP 302"));
    assert!(!error.contains("secret"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn permission_changes_during_a_probe_discard_tool_data() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    let reads = MockServer::start().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(call(
            "monitoring_probe",
            json!({"source":"health","request":{"kind":"http","path":"/health"}}),
        )))
        .expect(1)
        .mount(&http)
        .await;
    Mock::given(path("/health"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_json(json!({"private":"WITHDRAWN-PROBE-DATA"})),
        )
        .expect(1)
        .mount(&reads)
        .await;
    let mut s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    source(&mut s, &reads.uri());
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let (result, _) = tokio::join!(
        execute(
            &e,
            "probe-revoked",
            scope,
            900000,
            "Inspect",
            json!({}),
            true
        ),
        async {
            for _ in 0..100 {
                if !reads.received_requests().await.unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            assert!(!reads.received_requests().await.unwrap().is_empty());
            e.store.borrow().conn.execute("UPDATE telegram_config SET body=json_set(body,'$.version',2,'$.sources',json('[]'))",[]).unwrap();
        }
    );
    assert!(result.unwrap_err().contains("permission"));
    let run = e.store.borrow().ai_run("probe-revoked").unwrap().unwrap();
    assert!(!serde_json::to_string(&run)
        .unwrap()
        .contains("WITHDRAWN-PROBE-DATA"));
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn input_response_and_deadline_limits_fail_explicitly() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(131073)))
        .expect(1)
        .mount(&http)
        .await;
    let s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    assert!(execute(
        &e,
        "too-much-input",
        scope.clone(),
        900000,
        "Inspect",
        json!({"text":"x".repeat(65537)}),
        true
    )
    .await
    .unwrap_err()
    .contains("input size"));
    assert!(execute(
        &e,
        "deadline",
        scope.clone(),
        999,
        "Inspect",
        json!({}),
        true
    )
    .await
    .unwrap_err()
    .contains("deadline"));
    assert!(execute(
        &e,
        "too-much-output",
        scope,
        900000,
        "Inspect",
        json!({}),
        true
    )
    .await
    .unwrap_err()
    .contains("response size"));
    std::fs::remove_dir_all(dir).unwrap();
}
