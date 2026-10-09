use super::*;
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[test]
fn request_character_limit_counts_serialized_unicode_and_escaping() {
    let request = json!({"messages":[{"content":"é🙂\n\"\\".repeat(2000)}]});
    let body = request_body(&request, None).unwrap();
    let size = body.chars().count();
    assert!(body.len() > size);
    assert_eq!(request_body(&request, Some(size)).unwrap(), body);
    assert_eq!(
        request_body(&request, Some(size - 1)).unwrap_err(),
        REQUEST_CHARACTER_LIMIT_ERROR
    );
}

#[tokio::test]
async fn request_character_cap_is_enforced_before_http_and_includes_envelope() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer("done")))
        .expect(1)
        .mount(&http)
        .await;
    let s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    let engine = Engine::with_clock(s, Rc::new(|| 1000));
    let options = ExecutionOptions {
        max_tokens: 2048,
        max_turns: 1,
        max_request_chars: Some(10_000),
    };
    // The user text alone fits, but the complete request does not.
    assert!(execute_with_options(
        &engine,
        "too-long-classifier",
        scope.clone(),
        900000,
        "Classify",
        json!({"question":"a".repeat(9900)}),
        false,
        options
    )
    .await
    .unwrap_err()
    .contains(REQUEST_CHARACTER_LIMIT_ERROR));
    // Multibyte Unicode is counted in characters, not UTF-8 bytes.
    execute_with_options(
        &engine,
        "unicode-classifier",
        scope,
        900000,
        "Classify",
        json!({"question":"é".repeat(8000)}),
        false,
        options,
    )
    .await
    .unwrap();
    let requests = http.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let wire = std::str::from_utf8(&requests[0].body).unwrap();
    assert!(wire.chars().count() <= 10_000);
    assert!(wire.len() > 10_000);
    assert_eq!(
        engine
            .store
            .borrow()
            .ai_run("too-long-classifier")
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
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
    let d:crate::reports::Definition=serde_json::from_value(json!({"id":"morning","version":1,"enabled":false,"steps":[{"id":"private","kind":"script","source":"()=> 'unselected-marker'"},{"id":"facts","kind":"script","source":"()=> ({up:true})"},{"id":"analysis","kind":"analysis","instructions":"Summarize the facts","inputs":["facts"],"when":"ctx => ctx.steps.facts.value.up === true"}],"compose":"ctx=>({subject:'Morning',summary:ctx.steps.analysis.value.summary,sections:[]})","destinations":[]})).unwrap();
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
    assert_eq!(q[0]["tools"].as_array().unwrap().len(), 6);
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
        .respond_with(|r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            assert!(request.get("thinking_budget_tokens").is_none());
            ResponseTemplate::new(200).set_body_json(if request.get("tools").is_some() {
                call("monitoring_snapshot", json!({}))
            } else {
                assert!(
                    request["messages"].as_array().unwrap().last().unwrap()["content"]
                        .as_str()
                        .unwrap()
                        .contains("budget is exhausted")
                );
                answer("The recorded evidence is incomplete; the cause remains unverified.")
            })
        })
        .expect(6)
        .mount(&http)
        .await;
    let s = Store::open(dir.join("engine.db")).unwrap();
    let scope = telegram(&s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let result = execute(
        &e,
        "loop",
        scope.clone(),
        900000,
        "Investigate",
        json!({}),
        true,
    )
    .await
    .unwrap();
    assert_eq!(result["turns"], 6);
    assert!(result["summary"].as_str().unwrap().contains("unverified"));
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

#[tokio::test]
async fn long_report_budget_is_preserved_and_short_deadline_still_cancels() {
    let http = MockServer::start().await;
    let _dir = config(&http.uri());
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer("Ready")))
        .mount(&http).await;
    let s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    execute(&e, "long-budget", scope.clone(), 1_201_000, "Inspect", json!({}), false).await.unwrap();
    assert_eq!(e.store.borrow().ai_run("long-budget").unwrap().unwrap().deadline, 1_201_000);
    http.reset().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(200)).set_body_json(answer("Too late")))
        .mount(&http).await;
    assert!(execute(&e, "short-budget", scope, 1020, "Inspect", json!({}), false).await.is_err());
    assert_eq!(e.store.borrow().ai_run("short-budget").unwrap().unwrap().status, "failed");
    assert_eq!(http.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn conditional_report_analysis_skips_inference_and_rejects_invalid_conditions() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&http)
        .await;
    for (condition, optional, expected, output) in [
        ("ctx => false", false, "complete", "skipped"),
        ("ctx => 'false'", true, "partial", "failed"),
        (
            "ctx => { throw Error('bad condition'); }",
            false,
            "failed",
            "failed",
        ),
    ] {
        let mut s = Store::open(":memory:").unwrap();
        let d: crate::reports::Definition = serde_json::from_value(json!({
            "id":"conditional", "version":1, "enabled":false,
            "steps":[{"id":"analysis","kind":"analysis","instructions":"Assess",
                "inputs":[],"when":condition,"optional":optional}],
            "compose":"ctx=>({subject:'Nominal',summary:'',sections:[]})"
        }))
        .unwrap();
        s.report_save(&d, 0, 1000).unwrap();
        let run = s.report_start("conditional", "admin", false, 1000).unwrap();
        let e = Engine::with_clock(s, Rc::new(|| 1000));
        let w = crate::reports::worker::Worker::new(e.clone());
        w.advance(&run.id).await.unwrap();
        w.advance(&run.id).await.unwrap();
        let done = e.store.borrow().report_run(&run.id).unwrap();
        assert_eq!(done.status, expected);
        assert_eq!(done.outputs["analysis"].status, output);
        let count: i64 = e
            .store
            .borrow()
            .conn
            .query_row("SELECT count(*) FROM ai_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

fn host_value(seed: f64) -> Value {
    let series: Vec<Value> = (0..289).map(|i| if i < 8 { Value::Null } else { json!(seed + (i % 50) as f64) }).collect();
    crate::value::from_json(&json!({"reachable":true,"cpu":seed,"memory":61.2,"cpuHistory":series,"memoryHistory":series,
        "cpuMinuteHistory":series[..61].to_vec(),"note":"x".repeat(500),
        "disks":[{"id":"root","mount":"/","free":41.3,"availableBytes":193e9,"totalBytes":467e9}]}))
}
fn instance(s: &Store, id: &str, value: Value) {
    let i = crate::store::Instance { id: id.into(), definition: "host-snapshot".into(), params: json!({}), state: json!({}),
        value, has_value: true, timestamp: 1000, quality: "good".into(), revision: 1, generation: 1, history_count: 120, history_age_ms: 3_600_000 };
    s.conn.execute("INSERT OR IGNORE INTO definitions VALUES('host-snapshot','{}')", []).unwrap();
    s.conn.execute("INSERT INTO instances VALUES(?,?,?)", rusqlite::params![i.id, i.definition, serde_json::to_string(&i).unwrap()]).unwrap();
}

#[test]
fn value_preview_summarizes_series_and_clips_text() {
    let p = crate::value::preview(&host_value(10.0));
    assert_eq!(p["cpu"], 10.0);
    assert_eq!(p["cpuHistory"]["series"], 289);
    assert_eq!(p["cpuHistory"]["samples"], 281);
    assert_eq!(p["cpuHistory"]["max"], 59.0);
    assert_eq!(p["cpuHistory"]["min"], 10.0);
    assert!(p["cpuHistory"]["last"].is_number() && p["cpuHistory"]["mean"].is_number());
    assert_eq!(p["disks"][0]["mount"], "/");
    assert!(p["note"].as_str().unwrap().chars().count() <= 301);
}

#[tokio::test]
async fn snapshot_is_a_compact_catalogue_that_always_fits_the_tool_limit() {
    let s = Store::open(":memory:").unwrap();
    telegram(&s);
    for host in ["host-homelab", "host-vps-eu", "host-vps-us"] {
        instance(&s, host, host_value(20.0));
    }
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let v = tools::execute(&e, "monitoring_snapshot", json!({})).await.unwrap();
    let homelab = v["values"].as_array().unwrap().iter().find(|x| x["id"] == "host-homelab").unwrap();
    assert_eq!(homelab["value"]["cpuHistory"]["max"], 69.0);
    assert!(homelab.get("state").is_none());
    // Many large values still fit; the largest previews are replaced, not the catalogue.
    for i in 0..60 {
        instance(&e.store.borrow(), &format!("bulk-{i:02}"), host_value(i as f64));
    }
    let v = tools::execute(&e, "monitoring_snapshot", json!({})).await.unwrap();
    assert!(serde_json::to_vec(&v).unwrap().len() <= 30 * 1024);
    let values = v["values"].as_array().unwrap();
    assert_eq!(values.len(), 63);
    assert!(values.iter().any(|x| x["value"] == "omitted for size; use monitoring_read"));
}

#[tokio::test]
async fn tool_calls_cut_off_at_the_token_limit_are_reported_as_truncation() {
    let http = MockServer::start().await;
    let _dir = config(&http.uri());
    let mut cut = call("monitoring_snapshot", json!({}));
    cut["choices"][0]["finish_reason"] = json!("length");
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(cut))
        .mount(&http)
        .await;
    let s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let error = execute(&e, "cut-1", scope, 900000, "Check", json!({"question":"q"}), true).await.unwrap_err();
    assert!(error.contains("simple-ai output truncated at token limit"), "{error}");
    assert!(crate::conversation::failure_reason(&error).unwrap().contains("ran out of output space"));
}

#[tokio::test]
async fn text_form_tool_calls_continue_the_investigation_and_never_become_answers() {
    let http = MockServer::start().await;
    let _dir = config(&http.uri());
    let requests = Arc::new(Mutex::new(vec![]));
    let copy = requests.clone();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let mut q = copy.lock().unwrap();
            q.push(serde_json::from_slice::<Value>(&r.body).unwrap());
            ResponseTemplate::new(200).set_body_json(match q.len() {
                1 => answer("Let me look at the history first.\n\n<tool_call>\n<function=monitoring_read>\n<parameter=kind>\nhistory\n</parameter>\n<parameter=id>\nhost-homelab\n</parameter>\n</function>\n</tool_call>"),
                _ => answer("CPU peaked at 30.2% on homelab."),
            })
        })
        .mount(&http)
        .await;
    let s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    instance(&s, "host-homelab", host_value(20.0));
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let v = execute(&e, "text-calls-1", scope.clone(), 900000, "Investigate", json!({"question":"peak cpu?"}), true).await.unwrap();
    assert_eq!(v["summary"], "CPU peaked at 30.2% on homelab.");
    let q = requests.lock().unwrap();
    let history = q[1]["messages"].as_array().unwrap();
    let call = history.iter().find(|m| m["role"] == "assistant").unwrap();
    assert_eq!(call["tool_calls"][0]["function"]["name"], "monitoring_read");
    assert_eq!(call["content"], "Let me look at the history first.");
    assert!(history.iter().any(|m| m["role"] == "tool" && m["tool_call_id"].as_str().is_some_and(|id| id.starts_with("text-") && id == call["tool_calls"][0]["id"])));
    drop(q);

    // Markup that cannot be understood fails instead of reaching the user.
    let broken = MockServer::start().await;
    let _dir = config(&broken.uri());
    Mock::given(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer("thinking…\n<tool_call>\n<function=monitoring_read>\n<parameter=id>host")))
        .mount(&broken)
        .await;
    let error = execute(&e, "text-calls-2", scope, 900000, "Investigate", json!({"question":"q"}), true).await.unwrap_err();
    assert!(error.contains("simple-ai returned a malformed tool call"), "{error}");
    assert!(crate::conversation::failure_reason(&error).unwrap().contains("could not understand"));
}

fn acknowledgement_alert(s: &mut Store) {
    s.alert_observe(
        &crate::alerts::Observation {
            key: "ssh".into(),
            active: true,
            stage: "review".into(),
            severity: "critical".into(),
            message: "SSH configuration changed".into(),
            labels: Default::default(),
            reset_ack: false,
        },
        0,
        "monitor",
        100,
    )
    .unwrap();
}

#[tokio::test]
async fn acknowledgement_checks_scope_revision_and_audits_requesting_user() {
    let mut s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    acknowledgement_alert(&mut s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let args = json!({"key":"ssh","occurrence":1,"expected":1});
    let read = tools::execute(&e, "monitoring_read", json!({"kind":"alert","id":"ssh"}))
        .await
        .unwrap();
    assert_eq!(read["revision"], 1);
    assert!(tools::execute(&e, "alerts_acknowledge", args.clone())
        .await
        .is_err());
    assert!(tools::execute_scoped(
        &e,
        &Scope::Report {
            run: "report".into()
        },
        "alerts_acknowledge",
        args.clone()
    )
    .await
    .is_err());
    for bad in [
        json!({"key":"ssh","occurrence":2,"expected":1}),
        json!({"key":"ssh","occurrence":1,"expected":0}),
        json!({"key":"ssh","occurrence":1,"expected":1,"actor":"operator"}),
    ] {
        assert!(tools::execute_scoped(&e, &scope, "alerts_acknowledge", bad)
            .await
            .is_err());
    }
    assert!(e
        .store
        .borrow()
        .alert("ssh")
        .unwrap()
        .acknowledgement
        .is_none());
    let receipt = tools::execute_scoped(&e, &scope, "alerts_acknowledge", args.clone())
        .await
        .unwrap();
    assert_eq!(receipt["acknowledgement"]["actor"], "telegram:1:1");
    assert_eq!(receipt["active"], true);
    assert_eq!(receipt["revision"], 2);
    assert!(
        json!(e.store.borrow().alert_audit_history(Some("ssh")).unwrap())
            .to_string()
            .contains("telegram:1:1")
    );
    // An uncertain result cannot blindly mutate a newer revision.
    assert!(
        tools::execute_scoped(&e, &scope, "alerts_acknowledge", args)
            .await
            .is_err()
    );
    e.store
        .borrow()
        .conn
        .execute("DELETE FROM telegram_users", [])
        .unwrap();
    assert!(tools::execute_scoped(
        &e,
        &scope,
        "alerts_acknowledge",
        json!({"key":"ssh","occurrence":1,"expected":2})
    )
    .await
    .is_err());
}

#[tokio::test]
async fn acknowledgement_chat_authority_is_live_and_server_derived() {
    let mut s = Store::open(":memory:").unwrap();
    s.user_bootstrap("admin").unwrap();
    acknowledgement_alert(&mut s);
    let c = crate::chat::request(
        &mut s,
        "admin",
        "create",
        json!({"requestId":"ack-chat","text":"Acknowledge the SSH alert"}),
        1000,
    )
    .unwrap();
    let scope = Scope::Chat {
        request: c["request"].as_i64().unwrap(),
        session: c["session"].as_str().unwrap().into(),
        subject: "admin".into(),
    };
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let args = json!({"key":"ssh","occurrence":1,"expected":1});
    let receipt = tools::execute_scoped(&e, &scope, "alerts_acknowledge", args)
        .await
        .unwrap();
    assert_eq!(receipt["acknowledgement"]["actor"], "chat:admin");
    e.store
        .borrow()
        .conn
        .execute("UPDATE chat_requests SET status='cancelled'", [])
        .unwrap();
    assert!(tools::execute_scoped(
        &e,
        &scope,
        "alerts_acknowledge",
        json!({"key":"ssh","occurrence":1,"expected":2})
    )
    .await
    .is_err());
}

#[tokio::test]
async fn agent_tool_loop_can_acknowledge_exact_alert() {
    let http = MockServer::start().await;
    let dir = config(&http.uri());
    let count = Arc::new(Mutex::new(0));
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |_: &wiremock::Request| {
            let mut n = count.lock().unwrap();
            *n += 1;
            ResponseTemplate::new(200).set_body_json(if *n == 1 {
                call(
                    "alerts_acknowledge",
                    json!({"key":"ssh","occurrence":1,"expected":1}),
                )
            } else {
                answer("Acknowledged the SSH alert")
            })
        })
        .expect(2)
        .mount(&http)
        .await;
    let mut s = Store::open(":memory:").unwrap();
    let scope = telegram(&s);
    acknowledgement_alert(&mut s);
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let result = execute(
        &e,
        "ack-run",
        scope,
        900000,
        crate::conversation::ANSWER,
        json!({"question":"Acknowledge the SSH alert"}),
        true,
    )
    .await
    .unwrap();
    assert_eq!(result["summary"], "Acknowledged the SSH alert");
    assert_eq!(
        e.store
            .borrow()
            .alert("ssh")
            .unwrap()
            .acknowledgement
            .unwrap()
            .actor,
        "telegram:1:1"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
