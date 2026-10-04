use super::*;
use std::sync::{Arc, Mutex};
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

fn fixture() -> (Worker, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "talia-chat-{}",
        crate::telegram::transport::random().unwrap()
    ));
    std::fs::create_dir(&dir).unwrap();
    let mut s = Store::open(dir.join("engine.db")).unwrap();
    s.user_bootstrap("admin").unwrap();
    s.user_seen("viewer", "viewer").unwrap();
    (Worker::new(Engine::with_clock(s, Rc::new(|| 1000))), dir)
}
fn call(w: &Worker, op: &str, args: Value) -> Result<Value> {
    request(&mut w.engine.store.borrow_mut(), "admin", op, args, 1000)
}
fn create(w: &Worker, id: &str, text: &str) -> (String, i64) {
    let v = call(w, "create", json!({"requestId":id,"text":text})).unwrap();
    (
        v["session"].as_str().unwrap().into(),
        v["request"].as_i64().unwrap(),
    )
}
fn view(w: &Worker, session: &str) -> Value {
    call(w, "get", json!({"session":session})).unwrap()
}
/// Mock simple-ai: replies with `answers` in order and records request bodies.
async fn ai(answers: Vec<Value>) -> (MockServer, Arc<Mutex<Vec<Value>>>) {
    let server = MockServer::start().await;
    let seen = Arc::new(Mutex::new(vec![]));
    let (log, queue) = (seen.clone(), Arc::new(Mutex::new(answers)));
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            log.lock()
                .unwrap()
                .push(serde_json::from_slice::<Value>(&r.body).unwrap());
            let mut q = queue.lock().unwrap();
            let next = if q.len() > 1 {
                q.remove(0)
            } else {
                q[0].clone()
            };
            if next["status"].is_u64() {
                ResponseTemplate::new(next["status"].as_u64().unwrap() as u16)
            } else {
                ResponseTemplate::new(200).set_body_json(next)
            }
        })
        .mount(&server)
        .await;
    crate::ai::tests::config(&server.uri());
    (server, seen)
}
fn tool(name: &str, args: Value) -> Value {
    json!({"choices":[{"message":{"role":"assistant","tool_calls":[{"id":"call-1","type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":"tool_calls"}]})
}

#[test]
fn non_admins_are_forbidden_and_inputs_validated() {
    let (w, _) = fixture();
    let viewer = request(
        &mut w.engine.store.borrow_mut(),
        "viewer",
        "list",
        json!({}),
        1000,
    );
    assert_eq!(viewer.unwrap_err(), "forbidden");
    assert!(call(&w, "create", json!({"requestId":"bad id","text":"hi"})).is_err());
    assert!(call(&w, "create", json!({"requestId":"a","text":"   "})).is_err());
    assert!(call(
        &w,
        "create",
        json!({"requestId":"a","text":"x".repeat(8001)})
    )
    .is_err());
    assert_eq!(
        call(&w, "get", json!({"session":"chat-missing"})).unwrap_err(),
        "not_found"
    );
    assert!(call(&w, "unknown", json!({})).is_err());
}

#[test]
fn retried_create_and_send_never_duplicate() {
    let (w, _) = fixture();
    let first = create(&w, "c1", "Why is disk filling?\nmore detail");
    assert_eq!(first, create(&w, "c1", "Why is disk filling?\nmore detail"));
    let sessions = call(&w, "list", json!({})).unwrap();
    assert_eq!(sessions["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(sessions["sessions"][0]["title"], "Why is disk filling?");
    let send = json!({"session":first.0,"requestId":"s1","text":"and memory?"});
    let a = call(&w, "send", send.clone()).unwrap();
    assert_eq!(a, call(&w, "send", send).unwrap());
    assert_eq!(view(&w, &first.0)["requests"].as_array().unwrap().len(), 2);
    // Two active requests is the per-session bound.
    assert_eq!(
        call(
            &w,
            "send",
            json!({"session":first.0,"requestId":"s2","text":"third"})
        )
        .unwrap_err(),
        "limit_exceeded"
    );
}

#[tokio::test]
async fn answers_carry_session_context_and_report_tool_steps() {
    let (w, _) = fixture();
    let (_server, seen) = ai(vec![
        tool("monitoring_snapshot", json!({})),
        json!(crate::ai::tests::answer("Disk / is 41% free.")),
    ])
    .await;
    let (session, request) = create(&w, "c1", "How is the disk?");
    w.advance_session(&session).await.unwrap();
    let v = view(&w, &session);
    assert_eq!(v["requests"][0]["status"], "done");
    assert_eq!(v["requests"][0]["answer"], "Disk / is 41% free.");
    // Steps come from the recorded run, without raw tool output.
    let job: Job = serde_json::from_str(
        &w.engine
            .store
            .borrow()
            .conn
            .query_row(
                "SELECT body FROM chat_requests WHERE id=?",
                [request],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
    )
    .unwrap();
    let steps = w.engine.store.borrow().chat_steps(&job).unwrap();
    assert_eq!(
        steps,
        json!([{"label":"Checked the monitoring overview","done":true}])
    );
    assert!(seen.lock().unwrap()[0]["tools"].is_array());

    call(
        &w,
        "send",
        json!({"session":session,"requestId":"s1","text":"And yesterday?"}),
    )
    .unwrap();
    w.advance_session(&session).await.unwrap();
    let body = seen.lock().unwrap().last().unwrap().to_string();
    assert!(body.contains("Disk / is 41% free.") && body.contains("And yesterday?"));
    assert_eq!(view(&w, &session)["requests"][1]["status"], "done");
}

#[tokio::test]
async fn sessions_are_independent_and_stop_or_delete_only_affect_their_own() {
    let (w, _) = fixture();
    let (_server, seen) = ai(vec![json!(crate::ai::tests::answer("ok"))]).await;
    let (a, _) = create(&w, "a", "first session");
    let (b, _) = create(&w, "b", "second session");
    assert_eq!(
        call(&w, "stop", json!({"session":a})).unwrap()["stopped"],
        1
    );
    w.advance_session(&a).await.unwrap();
    w.advance_session(&b).await.unwrap();
    assert_eq!(view(&w, &a)["requests"][0]["status"], "stopped");
    assert_eq!(view(&w, &b)["requests"][0]["status"], "done");
    assert_eq!(seen.lock().unwrap().len(), 1);
    call(&w, "rename", json!({"session":b,"title":"Renamed"})).unwrap();
    call(&w, "delete", json!({"session":a})).unwrap();
    let list = call(&w, "list", json!({})).unwrap();
    assert_eq!(list["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(list["sessions"][0]["title"], "Renamed");
    assert_eq!(
        call(&w, "get", json!({"session":a})).unwrap_err(),
        "not_found"
    );
    // A deleted session cannot be resumed by another request.
    assert_eq!(
        call(&w, "send", json!({"session":a,"requestId":"x","text":"hi"})).unwrap_err(),
        "not_found"
    );
}

#[tokio::test]
async fn revoked_admins_are_cancelled_and_outages_get_a_short_reason() {
    let (w, _) = fixture();
    let (_server, _) = ai(vec![json!({"status":503})]).await;
    let (session, _) = create(&w, "c1", "status?");
    w.advance_session(&session).await.unwrap();
    let r = &view(&w, &session)["requests"][0];
    assert_eq!(r["status"], "failed");
    let error = r["error"].as_str().unwrap();
    assert!(
        error.contains("HTTP 503") && !error.contains("chat-"),
        "{error}"
    );

    let (other, _) = create(&w, "c2", "again");
    w.engine
        .store
        .borrow()
        .conn
        .execute(
            "UPDATE dashboard_users SET admin=0 WHERE subject='admin'",
            [],
        )
        .unwrap();
    w.advance_session(&other).await.unwrap();
    let status: String = w
        .engine
        .store
        .borrow()
        .conn
        .query_row(
            "SELECT status FROM chat_requests WHERE session=?",
            [&other],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "cancelled");
}

#[tokio::test]
async fn long_sessions_are_selectively_summarized_on_chat_tables() {
    let (w, _) = fixture();
    let server = MockServer::start().await;
    let summaries = Arc::new(Mutex::new(0));
    let counted = summaries.clone();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            let system = body["messages"][0]["content"].as_str().unwrap_or("");
            if system.contains("Selectively condense") {
                *counted.lock().unwrap() += 1;
                let input: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                let ids: Vec<Value> = input["entries"].as_array().unwrap().iter().map(|e| e["id"].clone()).collect();
                let (keep, rest) = ids.split_at(2);
                let selection = json!({"keep":keep,"summaries":[{"sources":rest,"text":"Earlier turns covered disk usage."}]});
                return ResponseTemplate::new(200).set_body_json(crate::ai::tests::answer(&selection.to_string()));
            }
            ResponseTemplate::new(200).set_body_json(crate::ai::tests::answer("ok"))
        })
        .mount(&server)
        .await;
    crate::ai::tests::config(&server.uri());
    let (session, _) = create(&w, "c0", "turn 0");
    w.advance_session(&session).await.unwrap();
    for i in 1..=9 {
        call(
            &w,
            "send",
            json!({"session":session,"requestId":format!("s{i}"),"text":format!("turn {i}")}),
        )
        .unwrap();
        w.advance_session(&session).await.unwrap();
    }
    assert!(*summaries.lock().unwrap() >= 1);
    let s = w.engine.store.borrow();
    let summary: i64 = s
        .conn
        .query_row(
            "SELECT count(*) FROM chat_context_entries WHERE session=? AND kind='summary'",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    let telegram: i64 = s
        .conn
        .query_row("SELECT count(*) FROM telegram_context_entries", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(summary >= 1);
    assert_eq!(telegram, 0);
    let done: i64 = s
        .conn
        .query_row(
            "SELECT count(*) FROM chat_requests WHERE session=? AND status='done'",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(done, 10);
}
