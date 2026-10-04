use super::*;
use crate::telegram::tests::{bot_fixture, connect, fixture, pair};
use std::sync::{Arc, Mutex};
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

fn ingest(w: &Worker, id: i64, text: &str) {
    w.engine
        .store
        .borrow_mut()
        .telegram_ingest(
            &json!({
        "update_id":id,"message":{"chat":{"id":55,"type":"private"},
        "from":{"id":55,"is_bot":false},"text":text}}),
            1000,
        )
        .unwrap();
}
fn job(w: &Worker, id: i64) -> Job {
    let text: String = w
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT body FROM telegram_jobs WHERE id=?", [id], |r| {
            r.get(0)
        })
        .unwrap();
    serde_json::from_str(&text).unwrap()
}
fn status(w: &Worker, id: i64) -> String {
    w.engine
        .store
        .borrow()
        .conn
        .query_row("SELECT status FROM telegram_jobs WHERE id=?", [id], |r| {
            r.get(0)
        })
        .unwrap()
}
fn payload(request: &Value) -> Value {
    serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap()
}
fn classification(decision: &str) -> String {
    json!({"decision":decision,"resume_pending":false,"clarification":null}).to_string()
}

#[tokio::test]
async fn oversized_classifier_preserves_cutoff_and_history_without_sending_request() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    seed(
        &w,
        1,
        "Only inspect staging; no active probes.",
        "Understood.",
    );
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            let system = request["messages"][0]["content"].as_str().unwrap();
            assert!(!system.contains(CLASSIFY));
            let input = payload(&request);
            assert!(input["history"]
                .to_string()
                .contains("Only inspect staging"));
            assert_eq!(input["context_unavailable"], false);
            assert_eq!(input["question"].as_str().unwrap().len(), 9900);
            response("Keeping the staging-only scope.")
        })
        .expect(1)
        .mount(&ai)
        .await;
    ingest(&w, 60, &"x".repeat(9900));
    w.conversation().await.unwrap();
    let j = job(&w, 60);
    assert_eq!(status(&w, 60), "done");
    assert!(j.context_fallback);
    assert!(!j.context_unavailable);
    assert_eq!(state(&w.engine.store.borrow(), &j).unwrap().cutoff, Some(1));
    assert!(j
        .maintenance_error
        .unwrap()
        .contains(ai::REQUEST_CHARACTER_LIMIT_ERROR));
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}
fn response(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(crate::ai::tests::answer(text))
}
async fn setup() -> (
    Worker,
    std::path::PathBuf,
    MockServer,
    MockServer,
    std::path::PathBuf,
) {
    let bot = bot_fixture().await;
    let (w, dir) = fixture();
    connect(&w).await;
    pair(&w, 55, 55, true).await;
    let ai = MockServer::start().await;
    let ai_dir = crate::ai::tests::config(&ai.uri());
    w.admin("admin",json!({"op":"telegramSettings","expected":1,"enabled":true,"investigations":true,"sources":[]})).await.unwrap();
    (w, dir, bot, ai, ai_dir)
}
fn seed(w: &Worker, id: i64, text: &str, answer: &str) {
    let j = Job {
        id,
        chat: 55,
        user: 55,
        epoch: 0,
        text: text.into(),
        report: None,
        created: 1000,
        deadline: 901000,
        revision: 2,
        phase: "answer".into(),
        through: 0,
        ai_run: None,
        error: None,
        context_version: 2,
        context_revision: 0,
        session_cutoff: Some(1),
        maintenance_error: None,
        context_fallback: false,
        context_unavailable: false,
        maintenance_deadline: None,
        batches: 0,
        offered: vec![],
        resumed: None,
    };
    record_turn(&w.engine.store.borrow(), &j, answer, 1000).unwrap();
    w.engine.store.borrow().conn.execute("INSERT OR IGNORE INTO telegram_working_context(chat,user,epoch,cutoff) VALUES(55,55,0,1)",[]).unwrap();
}

#[tokio::test]
async fn incident_new_topic_bypasses_old_compaction_and_retry_preserves_failed_outcome() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    for i in 1..=10 {
        seed(&w, i, "OLD-KNOT-QUESTION", &"OLD-KNOT-EVIDENCE ".repeat(10));
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let captured = seen.clone();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            let input = payload(&request);
            let system = request["messages"][0]["content"].as_str().unwrap();
            captured.lock().unwrap().push(request.clone());
            if system.contains(CLASSIFY) {
                assert!(request.get("tools").is_none());
                assert_eq!(request["max_tokens"], 2048);
                return response(&classification(if input["question"] == "Retry" {
                    "continue"
                } else {
                    "new_session"
                }));
            }
            assert!(!system.contains(SUMMARIZE));
            assert!(!input.to_string().contains("OLD-KNOT"));
            if input["question"] == "Investigate Pezzottify" {
                assert_eq!(input["history"], json!([]));
                ResponseTemplate::new(500)
            } else {
                assert!(input.to_string().contains("Investigate Pezzottify"));
                assert!(input.to_string().contains("outcome: failed"));
                response("The image route returned 503.")
            }
        })
        .mount(&ai)
        .await;
    ingest(&w, 60, "Investigate Pezzottify");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "failed");
    assert_eq!(
        state(&w.engine.store.borrow(), &job(&w, 60))
            .unwrap()
            .cutoff,
        Some(60)
    );
    ingest(&w, 61, "Retry");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 61), "done");
    assert_eq!(seen.lock().unwrap().len(), 4);
    let before: i64 = w
        .engine
        .store
        .borrow()
        .conn
        .query_row("SELECT count(*) FROM telegram_history", [], |r| r.get(0))
        .unwrap();
    w.conversation().await.unwrap();
    assert_eq!(before, 24);
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row("SELECT count(*) FROM telegram_history", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        before
    );
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn selective_compaction_keeps_exact_constraint_and_tracks_nonoverlapping_coverage() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    seed(&w, 1, "Never change threshold 0.012345.", "Acknowledged.");
    for i in 2..=9 {
        seed(
            &w,
            i,
            "Check image requests",
            &format!("noise-{i} {}", "repeated observation ".repeat(200)),
        );
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let captured = seen.clone();
    Mock::given(path("/v1/chat/completions")).respond_with(move |r:&wiremock::Request|{
        let request:Value=serde_json::from_slice(&r.body).unwrap();
        let input=payload(&request);
        captured.lock().unwrap().push(request.clone());
        let system=request["messages"][0]["content"].as_str().unwrap();
        if system.contains(CLASSIFY) {return response(&classification("continue"));}
        if system.contains(SUMMARIZE) {
            assert_eq!(request["max_tokens"],8192);assert!(request.get("tools").is_none());
            let mut keep=vec![];let mut sources=vec![];
            for e in input["entries"].as_array().unwrap() {
                if e.to_string().contains("0.012345") {keep.push(e["id"].clone());}
                else{sources.push(e["id"].clone());}
            }
            return response(&json!({"keep":keep,"summaries":[{"sources":sources,"text":"Repeated image checks; cause remains unconfirmed."}]}).to_string());
        }
        assert!(input.to_string().contains("0.012345"));
        assert!(input.to_string().contains("cause remains unconfirmed"));
        response("No change to the threshold.")
    }).mount(&ai).await;
    ingest(&w, 60, "Continue checking images");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "done");
    let j = job(&w, 60);
    assert!(j.batches > 0 && j.batches <= 2);
    let s = w.engine.store.borrow();
    let count: i64 = s
        .conn
        .query_row("SELECT count(*) FROM telegram_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 20);
    let mut q = s
        .conn
        .prepare(
            "SELECT id,covered FROM telegram_context_entries WHERE kind='summary' AND active=1",
        )
        .unwrap();
    let summaries = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap();
    let mut all = BTreeSet::new();
    for v in summaries {
        let (_, coverage) = v.unwrap();
        for id in serde_json::from_str::<Vec<String>>(&coverage).unwrap() {
            assert!(all.insert(id.clone()));
            assert_eq!(
                s.conn
                    .query_row(
                        "SELECT active FROM telegram_context_entries WHERE id=?",
                        [id],
                        |r| r.get::<_, bool>(0)
                    )
                    .unwrap(),
                false
            );
        }
    }
    drop(q);
    drop(s);
    // A subsequent short request does not reread the originals behind the summaries.
    ingest(&w, 61, "Continue");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 61), "done");
    let requests = seen.lock().unwrap();
    let last = &requests[requests.len() - 1];
    assert!(!payload(last).to_string().contains("noise-2 "));
    drop(requests);
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn ambiguous_session_is_clarified_before_tools_and_resumed_question_survives_retry() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    seed(&w, 1, "OLD topic", "OLD finding");
    let answers = Arc::new(Mutex::new(vec![]));
    let captured = answers.clone();
    Mock::given(path("/v1/chat/completions")).respond_with(move |r:&wiremock::Request|{
        let request:Value=serde_json::from_slice(&r.body).unwrap();
        let input=payload(&request);
        if request["messages"][0]["content"].as_str().unwrap().contains(CLASSIFY) {
            return response(&match input["question"].as_str().unwrap() {
                "Check that warning"=>json!({"decision":"clarify","resume_pending":false,"clarification":"The previous warning or a new one?"}),
                "A new image warning"=>json!({"decision":"new_session","resume_pending":true,"clarification":null}),
                _=>json!({"decision":"continue","resume_pending":false,"clarification":null}),
            }.to_string());
        }
        assert!(!input.to_string().contains("OLD"));
        captured.lock().unwrap().push(input.clone());
        if input["question"]=="A new image warning" {
            assert_eq!(input["resumed_question"]["question"],"Check that warning");
        }else{assert!(input.to_string().contains("Check that warning"));}
        response("Image warning checked.")
    }).mount(&ai).await;
    ingest(&w, 60, "Check that warning");
    w.conversation().await.unwrap();
    assert!(answers.lock().unwrap().is_empty());
    assert_eq!(job(&w, 60).phase, "clarification");
    assert!(state(&w.engine.store.borrow(), &job(&w, 60))
        .unwrap()
        .pending
        .is_some());
    ingest(&w, 61, "A new image warning");
    w.conversation().await.unwrap();
    assert_eq!(job(&w, 61).session_cutoff, Some(61));
    ingest(&w, 62, "Retry");
    w.conversation().await.unwrap();
    assert_eq!(answers.lock().unwrap().len(), 2);
    assert_eq!(status(&w, 62), "done");
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn truncated_compaction_falls_back_without_losing_history_or_failing_question() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    for i in 1..=9 {
        seed(&w, i, "image query", "HTTP 503");
    }
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            let system = request["messages"][0]["content"].as_str().unwrap();
            if system.contains(CLASSIFY) {
                return response(&classification("continue"));
            }
            if system.contains(SUMMARIZE) {
                let mut v = crate::ai::tests::answer("{incomplete");
                v["choices"][0]["finish_reason"] = json!("length");
                return ResponseTemplate::new(200).set_body_json(v);
            }
            assert!(payload(&request).to_string().contains("HTTP 503"));
            response("The last observation was HTTP 503.")
        })
        .mount(&ai)
        .await;
    ingest(&w, 60, "Check current image errors");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "done");
    assert!(job(&w, 60).maintenance_error.unwrap().contains("truncated"));
    let s = w.engine.store.borrow();
    assert_eq!(
        s.conn
            .query_row(
                "SELECT count(*) FROM telegram_context_entries WHERE active=0",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(s);
    ingest(&w, 61, "/compact");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 61), "failed");
    assert!(job(&w, 61).maintenance_error.is_some());
    let status = w
        .admin("admin", json!({"op":"telegramStatus"}))
        .await
        .unwrap();
    assert_eq!(status["jobs"][0]["phase"], "maintain");
    assert!(status["jobs"][0]["maintenanceError"]
        .as_str()
        .unwrap()
        .contains("truncated"));
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn classifier_failure_excludes_old_context_and_duplicate_update_is_idempotent() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    seed(&w, 1, "OLD question", "OLD hypothesis");
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            if request["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(CLASSIFY)
            {
                return response("invalid JSON");
            }
            let input = payload(&request);
            assert_eq!(input["history"], json!([]));
            assert_eq!(input["context_unavailable"], true);
            assert_eq!(input["question"], "Check Pezzottify now");
            response("Checking Pezzottify.")
        })
        .expect(2)
        .mount(&ai)
        .await;
    ingest(&w, 60, "Check Pezzottify now");
    ingest(&w, 60, "Check Pezzottify now");
    w.conversation().await.unwrap();
    w.conversation().await.unwrap();
    assert!(job(&w, 60).context_fallback);
    assert_eq!(
        state(&w.engine.store.borrow(), &job(&w, 60))
            .unwrap()
            .cutoff,
        Some(1)
    );
    assert_eq!(status(&w, 60), "done");
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn diagnostic_evidence_from_failed_runs_survives_ai_pruning_without_reasoning() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    let requests = Arc::new(Mutex::new(0));
    let count = requests.clone();
    Mock::given(path("/v1/chat/completions")).respond_with(move |r:&wiremock::Request|{
        let request:Value=serde_json::from_slice(&r.body).unwrap();
        if request["messages"][0]["content"].as_str().unwrap().contains(CLASSIFY) {
            return response(&classification(if payload(&request)["session_active"]==false{"new_session"}else{"continue"}));
        }
        let mut n=count.lock().unwrap();*n+=1;
        match *n {
            1=>ResponseTemplate::new(200).set_body_json(json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant",
                "content":"PRIVATE INTERNAL REASONING","tool_calls":[{"id":"probe-1","type":"function","function":{"name":"monitoring_read","arguments":"{\"kind\":\"variable\",\"id\":\"missing\"}"}}]}}]})),
            2=>ResponseTemplate::new(500),
            _=>{
                let input=payload(&request);
                assert!(!input.to_string().contains("PRIVATE INTERNAL REASONING"));
                assert!(input.to_string().contains("probe-1"));
                assert!(input.to_string().contains("monitoring_read"));
                assert!(input.to_string().contains("outcome: failed"));
                response("The earlier read failed; checking the target again.")
            }
        }
    }).mount(&ai).await;
    ingest(&w, 60, "Check images");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "failed");
    w.engine
        .store
        .borrow()
        .conn
        .execute(
            "DELETE FROM ai_runs WHERE status IN ('complete','failed')",
            [],
        )
        .unwrap();
    ingest(&w, 61, "Retry");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 61), "done");
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[test]
fn invalid_selections_cannot_drop_duplicate_or_forge_sources() {
    let entries = vec![
        Entry {
            id: "a".into(),
            request: 1,
            kind: "chat".into(),
            content: json!("exact"),
        },
        Entry {
            id: "b".into(),
            request: 1,
            kind: "evidence".into(),
            content: json!("evidence"),
        },
    ];
    for bad in [
        json!({"keep":["a"],"summaries":[]}),
        json!({"keep":["a","b","c"],"summaries":[]}),
        json!({"keep":["a"],"summaries":[{"sources":["a","b"],"text":"bad"}]}),
        json!({"keep":["a"],"summaries":[{"sources":["b"],"text":""}]}),
        json!({"keep":[],"summaries":[{"sources":["a","b"],"text":"x".repeat(9000)}]}),
    ] {
        let s: Selection = serde_json::from_value(bad).unwrap();
        assert!(validate_selection(&entries, &s).is_err());
    }
    let ok: Selection = serde_json::from_value(
        json!({"keep":["a"],"summaries":[{"sources":["b"],"text":"measured evidence"}]}),
    )
    .unwrap();
    validate_selection(&entries, &ok).unwrap();
}

#[test]
fn oversized_unicode_context_is_bounded_and_omissions_are_explicit() {
    let entries = (0..40)
        .map(|i| Entry {
            id: format!("m{i}"),
            request: i,
            kind: "chat".into(),
            content: json!({"text":"日".repeat(15000),"role":"user"}),
        })
        .collect::<Vec<_>>();
    let (result, omitted) = bounded(&entries, HISTORY_LIMIT);
    assert!(omitted);
    assert!(encoded(&result) <= HISTORY_LIMIT);
    assert!(result.iter().any(|e| e.request == 39));
    assert!(result.iter().all(|e| e.content["truncated"] == true));
}

#[tokio::test]
async fn completed_classifier_is_reused_after_restart_without_resubmission() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            if request["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(CLASSIFY)
            {
                response(&classification("new_session"))
            } else {
                response("Recovered answer.")
            }
        })
        .expect(2)
        .mount(&ai)
        .await;
    ingest(&w, 60, "Check current images");
    let j = job(&w, 60);
    ai::execute_with_options(
        &w.engine,
        "telegram-60-classify",
        j.scope(),
        121000,
        CLASSIFY,
        json!({"question":j.text}),
        false,
        ai::ExecutionOptions {
            max_tokens: 2048,
            max_turns: 1,
            max_request_chars: Some(CLASSIFIER_REQUEST_CHARS),
        },
    )
    .await
    .unwrap();
    drop(w);
    let s = Store::open(dir.join("engine.db")).unwrap();
    s.ai_recover().unwrap();
    let w = Worker::new(Engine::with_clock(s, Rc::new(|| 1000)));
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "done");
    assert_eq!(job(&w, 60).session_cutoff, Some(60));
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn reset_during_classification_discards_late_result() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    Mock::given(path("/v1/chat/completions"))
        .respond_with(
            response(&classification("new_session")).set_delay(Duration::from_millis(100)),
        )
        .expect(1)
        .mount(&ai)
        .await;
    ingest(&w, 60, "Check images");
    tokio::join!(
        async {
            w.conversation().await.unwrap();
        },
        async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            ingest(&w, 61, "/new");
        }
    );
    assert_eq!(status(&w, 60), "cancelled");
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row("SELECT count(*) FROM telegram_context_entries", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn upgrade_ignores_legacy_context_and_queue_order_does_not_import_future_questions() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    w.engine.store.borrow().conn.execute("INSERT OR REPLACE INTO telegram_context(chat,user,epoch,summary,through) VALUES(55,55,0,'LEGACY-SUMMARY',5)",[]).unwrap();
    w.engine.store.borrow().conn.execute("INSERT INTO telegram_history(chat,user,epoch,role,body) VALUES(55,55,0,'user','LEGACY-QUESTION')",[]).unwrap();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            let input = payload(&request);
            assert!(!input.to_string().contains("LEGACY"));
            if input["question"] == "FIRST" {
                assert!(!input.to_string().contains("SECOND"));
            }
            if request["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(CLASSIFY)
            {
                return response(&classification("new_session"));
            }
            assert_eq!(input["history"], json!([]));
            response("Checked.")
        })
        .expect(4)
        .mount(&ai)
        .await;
    ingest(&w, 60, "FIRST");
    ingest(&w, 61, "SECOND");
    w.conversation().await.unwrap();
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "done");
    assert_eq!(status(&w, 61), "done");
    assert_eq!(job(&w, 61).session_cutoff, Some(61));
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row(
                "SELECT summary FROM telegram_context WHERE chat=55 AND user=55",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "LEGACY-SUMMARY"
    );
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn explicit_report_reference_survives_new_session_without_importing_other_reports() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    {
        let mut s = w.engine.store.borrow_mut();
        let d:crate::reports::Definition=serde_json::from_value(json!({
            "id":"infra","version":1,"enabled":false,"steps":[{"id":"facts","kind":"script","source":"()=>1"}],
            "compose":"()=>({subject:'Infra',summary:'',sections:[]})","destinations":[]
        })).unwrap();
        s.report_save(&d, 0, 1000).unwrap();
        let mut r = s.report_start("infra", "admin", false, 1000).unwrap();
        r.status = "complete".into();
        r.text = Some("SELECTED IMAGE ERROR 503".into());
        s.report_put(&r).unwrap();
        s.conn.execute("INSERT INTO telegram_outbox(id,chat,kind,reference,status,body,message_id) VALUES('selected',55,'report',?,'sent','report',999)",[r.id]).unwrap();
    }
    seed(&w, 1, "OLD DNS", "OLD DNS evidence");
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            if request["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(CLASSIFY)
            {
                return response(&classification("new_session"));
            }
            let input = payload(&request);
            assert_eq!(input["history"], json!([]));
            assert_eq!(
                input["referenced_report"]["text"],
                "SELECTED IMAGE ERROR 503"
            );
            assert!(!input.to_string().contains("OLD DNS"));
            response("The selected report recorded HTTP 503.")
        })
        .expect(2)
        .mount(&ai)
        .await;
    w.engine.store.borrow_mut().telegram_ingest(&json!({"update_id":60,"message":{
        "chat":{"id":55,"type":"private"},"from":{"id":55,"is_bot":false},
        "text":"Explain this warning","reply_to_message":{"message_id":999,"text":"FORGED REPORT"}
    }}),1000).unwrap();
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "done");
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn maintenance_reconsiders_kept_messages_and_reuses_completed_batch_after_restart() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    seed(
        &w,
        1,
        "Keep exact constraint 0.012345",
        "Repeated discussion.",
    );
    ingest(&w, 60, "/compact");
    let mut j = job(&w, 60);
    let st = state(&w.engine.store.borrow(), &j).unwrap();
    let (entries, _) = active(&w.engine.store.borrow(), &j, &st).unwrap();
    j.offered = entries.iter().map(|e| e.id.clone()).collect();
    j.ai_run = Some("telegram-60-summarize-0".into());
    j.maintenance_deadline = Some(181000);
    w.engine
        .store
        .borrow()
        .telegram_job_put(&j, "running")
        .unwrap();
    let exact = entries
        .iter()
        .find(|e| e.content.to_string().contains("0.012345"))
        .unwrap()
        .id
        .clone();
    let other = entries.iter().find(|e| e.id != exact).unwrap().id.clone();
    let seen = Arc::new(Mutex::new(0));
    let count = seen.clone();
    Mock::given(path("/v1/chat/completions")).respond_with(move |r:&wiremock::Request|{
        let request:Value=serde_json::from_slice(&r.body).unwrap();
        assert!(request["messages"][0]["content"].as_str().unwrap().contains(SUMMARIZE));
        let mut n=count.lock().unwrap();*n+=1;
        if *n==1 {
            response(&json!({"keep":[exact],"summaries":[{"sources":[other],"text":"Discussion."}]}).to_string())
        }else{
            let entries=payload(&request)["entries"].as_array().unwrap().clone();
            assert!(entries.iter().any(|e|e["kind"]=="summary"));
            assert!(entries.iter().any(|e|e.to_string().contains("0.012345")));
            response(&json!({"keep":[],"summaries":[{"sources":entries.iter().map(|e|e["id"].clone()).collect::<Vec<_>>(),"text":"Constraint remains 0.012345; discussion resolved."}]}).to_string())
        }
    }).expect(2).mount(&ai).await;
    ai::execute_with_options(
        &w.engine,
        j.ai_run.as_ref().unwrap(),
        j.scope(),
        181000,
        SUMMARIZE,
        json!({"entries":entries}),
        false,
        ai::ExecutionOptions {
            max_tokens: 8192,
            max_turns: 1,
            max_request_chars: None,
        },
    )
    .await
    .unwrap();
    drop(w);
    let s = Store::open(dir.join("engine.db")).unwrap();
    s.ai_recover().unwrap();
    let w = Worker::new(Engine::with_clock(s, Rc::new(|| 1000)));
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "done");
    assert_eq!(*seen.lock().unwrap(), 1);
    ingest(&w, 61, "/compact");
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 61), "done");
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row(
                "SELECT count(*) FROM telegram_context_entries WHERE active=1 AND kind='chat'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn interrupted_maintenance_is_not_resubmitted_and_answer_still_runs() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    for i in 1..=9 {
        seed(&w, i, "Question", "Observation");
    }
    ingest(&w, 60, "Check now");
    let mut j = job(&w, 60);
    j.phase = "maintain".into();
    j.session_cutoff = Some(1);
    j.ai_run = Some("telegram-60-summarize-0".into());
    j.maintenance_deadline = Some(181000);
    w.engine
        .store
        .borrow()
        .telegram_job_put(&j, "running")
        .unwrap();
    let r = ai::Run {
        id: j.ai_run.clone().unwrap(),
        scope: j.scope(),
        created: 1000,
        deadline: 181000,
        status: "running".into(),
        model: "fixture-model".into(),
        messages: vec![],
        turns: 1,
        tools: false,
        summary: None,
        error: None,
        usage: vec![],
    };
    w.engine
        .store
        .borrow()
        .conn
        .execute(
            "INSERT INTO ai_runs VALUES(?,?,?,?)",
            params![
                r.id,
                r.status,
                r.created,
                serde_json::to_string(&r).unwrap()
            ],
        )
        .unwrap();
    drop(w);
    let s = Store::open(dir.join("engine.db")).unwrap();
    s.ai_recover().unwrap();
    let w = Worker::new(Engine::with_clock(s, Rc::new(|| 1000)));
    Mock::given(path("/v1/chat/completions"))
        .respond_with(move |r: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&r.body).unwrap();
            assert!(request["tools"].is_array());
            assert_eq!(payload(&request)["question"], "Check now");
            response("Checked.")
        })
        .expect(1)
        .mount(&ai)
        .await;
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "done");
    assert!(job(&w, 60)
        .maintenance_error
        .unwrap()
        .contains("interrupted"));
    assert!(job(&w, 60).context_fallback);
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[tokio::test]
async fn expired_maintenance_budget_falls_back_and_revocation_discards_a_summary() {
    let (w, dir, _bot, ai, ai_dir) = setup().await;
    for i in 1..=9 {
        seed(&w, i, "Question", "Observation");
    }
    ingest(&w, 60, "Check now");
    let mut j = job(&w, 60);
    j.phase = "maintain".into();
    j.session_cutoff = Some(1);
    j.maintenance_deadline = Some(999);
    w.engine
        .store
        .borrow()
        .telegram_job_put(&j, "queued")
        .unwrap();
    Mock::given(path("/v1/chat/completions"))
        .respond_with(response("Checked."))
        .expect(1)
        .mount(&ai)
        .await;
    w.conversation().await.unwrap();
    assert_eq!(status(&w, 60), "done");
    assert!(job(&w, 60).maintenance_error.is_some());
    ai.verify().await;
    ai.reset().await;

    ingest(&w, 61, "/compact");
    Mock::given(path("/v1/chat/completions")).respond_with(move |r:&wiremock::Request|{
        let request:Value=serde_json::from_slice(&r.body).unwrap();
        let ids=payload(&request)["entries"].as_array().unwrap().iter().map(|e|e["id"].clone()).collect::<Vec<_>>();
        response(&json!({"keep":[],"summaries":[{"sources":ids,"text":"Summary that must not commit"}]}).to_string())
            .set_delay(Duration::from_millis(100))
    }).expect(1).mount(&ai).await;
    tokio::join!(
        async {
            w.conversation().await.unwrap();
        },
        async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            w.engine
                .store
                .borrow()
                .conn
                .execute("DELETE FROM telegram_users WHERE id=55", [])
                .unwrap();
        }
    );
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row(
                "SELECT count(*) FROM telegram_context_entries WHERE kind='summary'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        w.engine
            .store
            .borrow()
            .conn
            .query_row(
                "SELECT count(*) FROM telegram_outbox WHERE id LIKE 'compact-61-%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    ai.verify().await;
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(ai_dir).unwrap();
}

#[test]
fn migration_preserves_original_history_and_legacy_job_shape() {
    let (w, dir) = fixture();
    {
        let s = w.engine.store.borrow();
        s.conn.execute_batch("DROP TABLE telegram_working_context; DROP TABLE telegram_context_entries; DROP TABLE telegram_history_requests;
            INSERT INTO telegram_context(chat,user,summary,through) VALUES(55,55,'legacy summary',1);
            INSERT INTO telegram_history(chat,user,epoch,role,body) VALUES(55,55,0,'user','legacy question');
            INSERT INTO telegram_jobs VALUES(60,55,55,'queued','{\"id\":60,\"chat\":55,\"user\":55,\"epoch\":0,\"text\":\"legacy question\",\"report\":null,\"created\":1000,\"deadline\":901000,\"revision\":2,\"phase\":\"answer\",\"through\":0,\"ai_run\":null,\"error\":null}');
            PRAGMA user_version=17;").unwrap();
    }
    drop(w);
    let s = Store::open(dir.join("engine.db")).unwrap();
    assert_eq!(
        s.conn
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        20
    );
    assert_eq!(
        s.conn
            .query_row("SELECT count(*) FROM telegram_context_entries", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
    let body: String = s
        .conn
        .query_row("SELECT body FROM telegram_jobs", [], |r| r.get(0))
        .unwrap();
    let j: Job = serde_json::from_str(&body).unwrap();
    assert_eq!(j.context_version, 0);
    assert_eq!(
        s.conn
            .query_row("SELECT body FROM telegram_history", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "legacy question"
    );
    assert_eq!(
        s.conn
            .query_row("SELECT summary FROM telegram_context", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "legacy summary"
    );
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stale_context_revision_cannot_replace_another_commit() {
    let (w, dir) = fixture();
    seed(&w, 1, "Question", "Answer");
    let j = Job {
        id: 60,
        chat: 55,
        user: 55,
        epoch: 0,
        text: "Question".into(),
        report: None,
        created: 1000,
        deadline: 901000,
        revision: 2,
        phase: "answer".into(),
        through: 0,
        ai_run: None,
        error: None,
        context_version: 2,
        context_revision: 0,
        session_cutoff: Some(1),
        maintenance_error: None,
        context_fallback: false,
        context_unavailable: false,
        maintenance_deadline: None,
        batches: 0,
        offered: vec![],
        resumed: None,
    };
    let mut st = state(&w.engine.store.borrow(), &j).unwrap();
    put_state(&w.engine.store.borrow(), &j, &st).unwrap();
    st.cutoff = Some(60);
    assert!(put_state(&w.engine.store.borrow(), &j, &st)
        .unwrap_err()
        .contains("revision"));
    assert_eq!(state(&w.engine.store.borrow(), &j).unwrap().cutoff, Some(1));
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn context_preserves_turn_order_and_fitting_exact_messages() {
    let entries = vec![
        Entry {
            id: "message-10".into(),
            request: 1,
            kind: "chat".into(),
            content: json!({"role":"assistant","text":"answer"}),
        },
        Entry {
            id: "message-9".into(),
            request: 1,
            kind: "chat".into(),
            content: json!({"role":"user","text":"q".repeat(16000)}),
        },
    ];
    let (result, omitted) = bounded(&entries, HISTORY_LIMIT);
    assert!(!omitted);
    assert_eq!(result[0].id, "message-9");
    assert_eq!(result[0].content["text"].as_str().unwrap().len(), 16000);
    assert_eq!(result[1].id, "message-10");
}
