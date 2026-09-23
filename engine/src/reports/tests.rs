use super::*;
use crate::{
    authority::{Action as Permission, Family, Grant, Scope},
    reports::worker::Worker,
    runtime::Engine,
};
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};
fn definition() -> Definition {
    serde_json::from_value(json!({"id":"morning","version":1,"enabled":false,"schedule":{"kind":"interval","every_ms":60000},"steps":[{"id":"check","kind":"script","source":"ctx=>({healthy:true,label:'<script>bad</script>'})"}],"compose":"ctx=>({subject:'Morning report',summary:'All checked',sections:[{title:'Status',text:ctx.steps.check.value.label}]})","destinations":[]})).unwrap()
}
fn operator(s: &mut Store) -> crate::authority::Session {
    s.agent_policy_set(
        "report-admin",
        0,
        true,
        &[Grant {
            family: Family::Authoring,
            actions: [Permission::Save].into_iter().collect(),
            scope: Scope::All,
        }],
    )
    .unwrap();
    let token = s.agent_credential_issue("report-admin").unwrap();
    s.agent_authenticate(&token).unwrap()
}
#[test]
fn definition_cas_admission_deduplication_and_authority() {
    let mut s = Store::open(":memory:").unwrap();
    let actor = operator(&mut s);
    let mut d = definition();
    d.enabled = true;
    d.destinations = vec!["mail".into()];
    s.alert_destination_save(
        &crate::alerts::delivery::Destination {
            id: "mail".into(),
            version: 1,
            channel: "email".into(),
            provider: "mail".into(),
            target: "owner@example.test".into(),
            enabled: true,
        },
        0,
        "operator",
        1000,
    )
    .unwrap();
    let save = json!({"definition":d,"expected":0,"requestId":"save-1"});
    let first = s.report_api(&actor, "save", save.clone(), 1000).unwrap();
    assert_eq!(first, s.report_api(&actor, "save", save, 1000).unwrap());
    assert!(s.report_save(&d, 0, 1000).is_err());
    let args = json!({"id":"morning","send":false,"requestId":"run-1"});
    let run = s.report_api(&actor, "run", args.clone(), 1000).unwrap();
    assert_eq!(run, s.report_api(&actor, "run", args, 2000).unwrap());
    assert!(s.report_start("morning", "admin", false, 2000).is_err());
    assert!(s
        .report_api(
            &actor,
            "run",
            json!({"id":"morning","send":true,"requestId":"run-1"}),
            2000
        )
        .is_err());
    let viewer = s.browser_alert_session("viewer").unwrap();
    assert!(s.report_api(&viewer, "list", json!({}), 0).is_err());
    let mut next = d.clone();
    next.version = 2;
    next.compose = "ctx=>({subject:'Updated',summary:'new',sections:[]})".into();
    s.report_save(&next, 1, 2000).unwrap();
    assert_eq!(
        s.report_run(run["run_id"].as_str().unwrap())
            .unwrap()
            .definition
            .version,
        1
    );
    s.report_scheduled(200000).unwrap();
    assert_eq!(s.report_active().unwrap().len(), 1);
    let due: i64 = s
        .conn
        .query_row("SELECT next_due FROM report_definitions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(due, 260000);
}
#[tokio::test]
async fn preview_optional_failure_safe_html_and_script_budget() {
    let mut s = Store::open(":memory:").unwrap();
    let mut d = definition();
    d.steps.push(serde_json::from_value(json!({"id":"broken","kind":"script","source":"()=>{throw Error('probe failed')}","optional":true})).unwrap());
    s.report_save(&d, 0, 1000).unwrap();
    let r = s.report_start("morning", "operator", false, 1000).unwrap();
    let engine = Engine::with_clock(s, Rc::new(|| 1000));
    let worker = Worker::new(engine.clone());
    for _ in 0..3 {
        worker.advance(&r.id).await.unwrap();
    }
    let r = engine.store.borrow().report_run(&r.id).unwrap();
    assert_eq!(r.status, "partial");
    assert!(r.deliveries.is_empty());
    let html = r.html.unwrap();
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("Incomplete step broken"));
    assert!(evaluate("()=>{while(true){}}", &json!({})).is_err());
    assert!(evaluate("async()=>42", &json!({})).is_err());
    assert_eq!(
        evaluate(
            "ctx=>ctx.decode({version:1,value:['number',42]})",
            &json!({})
        )
        .unwrap(),
        42
    );
}
fn tmp() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "talia-reports-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
#[tokio::test]
async fn delivery_recovery_is_explicit() {
    let mut s = Store::open(":memory:").unwrap();
    s.report_save(&definition(), 0, 1000).unwrap();
    let mut r = s.report_start("morning", "operator", false, 1000).unwrap();
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let w = Worker::new(e.clone());
    r.status = "delivering".into();
    r.deliveries = vec![Delivery {
        destination: "mail".into(),
        version: 1,
        status: "sending".into(),
        attempts: 1,
        next_at: 0,
        error: None,
    }];
    e.store.borrow().report_put(&r).unwrap();
    for i in 0..101 {
        let mut other = r.clone();
        other.id = format!("recovery-{i}");
        e.store.borrow().report_put(&other).unwrap();
    }
    e.store.borrow().report_recover().unwrap();
    assert_eq!(
        e.store
            .borrow()
            .report_run("recovery-100")
            .unwrap()
            .deliveries[0]
            .status,
        "unknown"
    );
    assert_eq!(
        e.store.borrow().report_run(&r.id).unwrap().deliveries[0].status,
        "unknown"
    );
    w.advance(&r.id).await.unwrap();
    assert_eq!(
        e.store.borrow().report_run(&r.id).unwrap().status,
        "partial"
    );
}
#[tokio::test]
async fn http_inputs_and_multipart_smtp_delivery() {
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };
    let http = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"up":true})))
        .expect(1)
        .mount(&http)
        .await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured = Arc::new(Mutex::new(String::new()));
    let mail = captured.clone();
    let smtp = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read, mut write) = socket.into_split();
        let mut reader = BufReader::new(read);
        write.write_all(b"220 fixture ESMTP\r\n").await.unwrap();
        let mut data = false;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            if data {
                if line == ".\r\n" {
                    data = false;
                    write.write_all(b"250 accepted\r\n").await.unwrap();
                } else {
                    mail.lock().unwrap().push_str(&line);
                }
                continue;
            }
            let reply: &[u8] = if line.starts_with("DATA") {
                data = true;
                b"354 data\r\n"
            } else if line.starts_with("QUIT") {
                write.write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                b"250 ok\r\n"
            };
            write.write_all(reply).await.unwrap();
        }
    });
    let dir = tmp();
    let config = dir.join("providers.json");
    std::fs::write(&config,json!({"mail":{"kind":"smtp","host":"127.0.0.1","port":port,"tls":"none_loopback","from":"talia@example.test"}}).to_string()).unwrap();
    let mut s = Store::open(":memory:").unwrap();
    let mut monitoring = s.monitoring_config().unwrap();
    let expected = monitoring.version;
    monitoring.version += 1;
    monitoring.sources.push(crate::monitoring::DataSource {
        id: "health".into(),
        kind: "http".into(),
        url: http.uri(),
        credential_ref: None,
        timeout_ms: 1000,
        max_bytes: 4096,
    });
    s.configure_monitoring(&monitoring, expected).unwrap();
    s.alert_destination_save(
        &crate::alerts::delivery::Destination {
            id: "mail".into(),
            version: 1,
            channel: "email".into(),
            provider: "mail".into(),
            target: "owner@example.test".into(),
            enabled: true,
        },
        0,
        "operator",
        1000,
    )
    .unwrap();
    let mut d = definition();
    d.steps=vec![serde_json::from_value(json!({"id":"health","kind":"source","source":"health","request":"ctx=>({kind:'http',path:'/health',method:'GET'})"})).unwrap()];
    d.compose="ctx=>({subject:'Morning health',summary:ctx.decode(ctx.steps.health.value.wire).up?'Healthy':'Down',sections:[]})".into();
    d.destinations = vec!["mail".into()];
    s.report_save(&d, 0, 1000).unwrap();
    let actor = operator(&mut s);
    let r = s.report_start("morning", "operator", false, 1000).unwrap();
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let mut worker = Worker::new(e.clone());
    worker.providers = Some(config.to_str().unwrap().into());
    worker.advance(&r.id).await.unwrap();
    worker.advance(&r.id).await.unwrap();
    assert_eq!(
        e.store.borrow().report_run(&r.id).unwrap().status,
        "complete"
    );
    let request = json!({"id":r.id,"requestId":"deliver-1"});
    let response = e
        .store
        .borrow_mut()
        .report_api(&actor, "deliver", request.clone(), 1000)
        .unwrap();
    assert_eq!(
        response,
        e.store
            .borrow_mut()
            .report_api(&actor, "deliver", request, 1000)
            .unwrap()
    );
    worker.advance(&r.id).await.unwrap();
    let done = e.store.borrow().report_run(&r.id).unwrap();
    assert_eq!(done.deliveries[0].status, "sent");
    assert_eq!(done.status, "complete");
    smtp.await.unwrap();
    let body = captured.lock().unwrap();
    assert!(body.contains("multipart/alternative"));
    assert!(body.contains("text/html"));
    assert!(body.contains("text/plain"));
    assert!(body.contains("Healthy"));
    drop(body);
    assert!(e
        .store
        .borrow_mut()
        .report_api(
            &actor,
            "deliver",
            json!({"id":r.id,"requestId":"deliver-2"}),
            1000
        )
        .is_err());
    http.verify().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn retired_integration_migration_preserves_history_and_delivery() {
    let dir = tmp();
    let db = dir.join("engine.db");
    let mut s = Store::open(&db).unwrap();
    s.report_save(&definition(), 0, 1000).unwrap();
    let r = s.report_start("morning", "operator", false, 1000).unwrap();
    let legacy_step = json!({"id":"analysis","kind":"simple_agents","provider":"old-profile","instructions":"Assess","inputs":["check"]});
    let mut old_definition = serde_json::to_value(definition()).unwrap();
    old_definition["steps"]
        .as_array_mut()
        .unwrap()
        .push(legacy_step.clone());
    old_definition["enabled"] = json!(true);
    s.conn
        .execute(
            "UPDATE report_definitions SET body=?,next_due=2000 WHERE id='morning'",
            [old_definition.to_string()],
        )
        .unwrap();
    for (id, status) in [
        ("pending", "running"),
        ("done", "complete"),
        ("delivery", "delivering"),
    ] {
        let mut old_run = serde_json::to_value(&r).unwrap();
        old_run["id"] = json!(id);
        old_run["definition"] = old_definition.clone();
        old_run["status"] = json!(status);
        old_run["agent"] = json!({"session_id":"old-session","request":{"key":"original"}});
        old_run["outputs"] = json!({"analysis":{"status":"succeeded","value":{"result":{"summary":"Kept evidence"}},"error":null}});
        old_run["content"] =
            json!({"subject":"Kept report","summary":"Already composed","sections":[]});
        s.conn
            .execute(
                "INSERT INTO report_runs VALUES(?,?,?,1000,?)",
                params![id, "morning", status, old_run.to_string()],
            )
            .unwrap();
    }
    s.conn.execute("INSERT INTO telegram_config VALUES(1,?)",[json!({"version":4,"enabled":true,"bot_id":123,"username":"bot","token":"encrypted-token","offset":71,"observer":"old-profile","sources":["prometheus"],"last_poll":1000,"error":null}).to_string()]).unwrap();
    s.conn.execute("INSERT INTO telegram_peers VALUES(42,?)",[json!({"chat":42,"name":"Private","kind":"private","delivery":true,"investigate":true}).to_string()]).unwrap();
    s.conn
        .execute(
            "INSERT INTO telegram_jobs VALUES(7,42,42,'running',?)",
            [json!({"pending":{"session_id":"old-session"}}).to_string()],
        )
        .unwrap();
    s.conn.execute("INSERT INTO telegram_history(chat,user,epoch,role,body) VALUES(42,42,0,'assistant','Past answer')",[]).unwrap();
    s.conn.execute("INSERT INTO telegram_outbox(id,chat,kind,status,body) VALUES('chat',42,'chat','pending','Old answer'),('report',42,'report','pending','Composed report')",[]).unwrap();
    s.conn.execute_batch("DROP TABLE ai_account; CREATE TABLE telegram_observer(id INTEGER PRIMARY KEY,hash TEXT); INSERT INTO telegram_observer VALUES(1,'old-secret-hash'); PRAGMA user_version=14;").unwrap();
    drop(s);
    let mut s = Store::open(&db).unwrap();
    assert_eq!(
        s.conn
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        17
    );
    let d = s.report_definition("morning").unwrap();
    assert!(!d.enabled);
    assert_eq!(d.version, 2);
    match &d.steps[1].action {
        Action::Unavailable { original, .. } => assert_eq!(original, &legacy_step),
        _ => panic!("step was not retired"),
    }
    assert!(s.report_start("morning", "operator", false, 3000).is_err());
    assert!(d.validate(&s).is_err());
    assert!(serde_json::from_value::<Step>(legacy_step).is_err());
    let pending = s.report_run("pending").unwrap();
    assert_eq!(pending.status, "failed");
    assert_eq!(
        pending.retired_execution.unwrap()["session_id"],
        "old-session"
    );
    assert_eq!(
        s.report_run("done").unwrap().content.unwrap().summary,
        "Already composed"
    );
    assert_eq!(
        s.report_run("done").unwrap().outputs["analysis"].value["result"]["summary"],
        "Kept evidence"
    );
    assert_eq!(s.report_run("delivery").unwrap().status, "delivering");
    // A normal run is unaffected and remains executable.
    assert_eq!(s.report_run(&r.id).unwrap().status, "queued");
    let c = s.telegram_config().unwrap();
    assert_eq!(c.token, "encrypted-token");
    assert_eq!(c.offset, 71);
    assert!(c.enabled);
    assert_eq!(
        s.conn
            .query_row("SELECT status FROM telegram_jobs WHERE id=7", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "cancelled"
    );
    assert_eq!(
        s.conn
            .query_row("SELECT body FROM telegram_history", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "Past answer"
    );
    assert_eq!(
        s.conn
            .query_row(
                "SELECT status FROM telegram_outbox WHERE id='report'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "pending"
    );
    assert_eq!(
        s.conn
            .query_row(
                "SELECT status FROM telegram_outbox WHERE id='chat'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "failed"
    );
    assert!(s.conn.prepare("SELECT * FROM telegram_observer").is_err());
    drop(s);
    // Reopening does not repeat the version bump or lose the archived step.
    let s = Store::open(&db).unwrap();
    assert_eq!(s.report_definition("morning").unwrap().version, 2);
    let w = Worker::new(Engine::with_clock(s, Rc::new(|| 3000)));
    w.advance(&r.id).await.unwrap();
    w.advance(&r.id).await.unwrap();
    assert_eq!(
        w.engine.store.borrow().report_run(&r.id).unwrap().status,
        "complete"
    );
    drop(w);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn analysis_inspection_is_admin_only_and_pruning_preserves_active_work() {
    let schema=api::tools().into_iter().find(|t|t["name"]=="reports_analysis_get").unwrap();
    assert_eq!(schema["inputSchema"]["required"],json!(["id"]));
    assert_eq!(schema["annotations"]["readOnlyHint"],true);
    let mut s = Store::open(":memory:").unwrap();
    let actor = operator(&mut s);
    s.report_save(&definition(), 0, 1000).unwrap();
    let mut parent = s.report_start("morning", "admin", false, 1000).unwrap();
    let ai = crate::ai::Run {
        id: "ai-result".into(),
        scope: crate::ai::Scope::Report {
            run: parent.id.clone(),
        },
        created: 1000,
        deadline: 5000,
        status: "complete".into(),
        model: "fixture".into(),
        messages: vec![],
        turns: 1,
        tools: false,
        summary: Some("Result".into()),
        error: None,
        usage: vec![],
    };
    s.conn
        .execute(
            "INSERT INTO ai_runs VALUES('ai-result','complete',1000,?)",
            [serde_json::to_string(&ai).unwrap()],
        )
        .unwrap();
    assert_eq!(
        s.report_api(&actor, "analysis_get", json!({"id":"ai-result"}), 2000)
            .unwrap()["run"]["summary"],
        "Result"
    );
    let viewer = s.browser_alert_session("viewer").unwrap();
    assert!(s
        .report_api(&viewer, "analysis_get", json!({"id":"ai-result"}), 2000)
        .is_err());
    assert_eq!(
        s.report_api(
            &actor,
            "prune",
            json!({"before":2000,"requestId":"keep-active"}),
            2000
        )
        .unwrap()["ai_removed"],
        0
    );
    parent.status = "complete".into();
    s.report_put(&parent).unwrap();
    let args = json!({"before":2000,"requestId":"remove-finished"});
    let result = s.report_api(&actor, "prune", args.clone(), 2000).unwrap();
    assert_eq!(result["ai_removed"], 1);
    assert_eq!(result, s.report_api(&actor, "prune", args, 2000).unwrap());
    assert!(s.ai_run("ai-result").unwrap().is_none());
}
