use super::*;
use crate::{
    authority::{Action as Permission, Family, Grant, Scope},
    reports::worker::Worker,
    runtime::Engine,
};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
};
use wiremock::{
    matchers::{method, path, path_regex},
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
async fn agent_submission_reconciles_after_restart_and_composes_retained_result() {
    agent_recovery(503).await;
    agent_recovery(201).await;
}
async fn agent_recovery(submission_status: u16) {
    let server = MockServer::start().await;
    let dir = tmp();
    let token = dir.join("token");
    std::fs::write(&token, "fixture-credential").unwrap();
    let request: Value = serde_json::from_str(include_str!(
        "../../vendor/simple-agents-client/contracts/v1/examples/observe.json"
    ))
    .unwrap();
    let provider = json!({"origin":server.uri(),"caller_id":"talia","token_file":token,"profile_id":request["profile_id"],"capabilities":[],"binding_ids":[],"budget":request["budget"]});
    let config = dir.join("agents.json");
    std::fs::write(&config, json!({"observer":provider}).to_string()).unwrap();
    let submitted = Arc::new(Mutex::new(None::<Value>));
    let received = submitted.clone();
    Mock::given(method("POST"))
        .and(path("/v1/sessions"))
        .respond_with(move |r: &wiremock::Request| {
            *received.lock().unwrap() = Some(serde_json::from_slice(&r.body).unwrap());
            ResponseTemplate::new(submission_status)
        })
        .expect(1)
        .mount(&server)
        .await;
    let received = submitted.clone();
    Mock::given(method("GET")).and(path_regex("/v1/sessions/by-key/.*")).respond_with(move|_:&wiremock::Request|{
  if let Some(r)=received.lock().unwrap().as_ref(){ResponseTemplate::new(200).set_body_json(json!({"version":1,"session_id":"session-1","caller_id":"talia","source":r["source"],"profile_id":r["profile_id"],"state":"succeeded","resource_version":2,"attempt":1,"effective":{"profile_revision":1,"engine":"fixture","engine_version":"1","capabilities":[],"binding_revisions":[],"budget":r["budget"]},"created_at_ms":1000,"retain_until_ms":99999999}))}else{ResponseTemplate::new(404)}
 }).mount(&server).await;
    Mock::given(method("GET")).and(path("/v1/sessions/session-1/result")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"outcome":"succeeded","summary":"Agent found all services healthy","artifact_ids":[],"effect_receipts":[]}))).mount(&server).await;
    let db = dir.join("engine.db");
    let mut s = Store::open(&db).unwrap();
    let mut d = definition();
    d.steps.push(serde_json::from_value(json!({"id":"analysis","kind":"simple_agents","provider":"observer","instructions":"Assess these observations","inputs":["check"]})).unwrap());
    d.compose="ctx=>({subject:'Morning report',summary:ctx.steps.analysis.value.result.summary,sections:[]})".into();
    s.report_save(&d, 0, 1000).unwrap();
    let r = s.report_start("morning", "operator", false, 1000).unwrap();
    let clock = Rc::new(Cell::new(1000));
    let e = Engine::with_clock(s, {
        let c = clock.clone();
        Rc::new(move || c.get())
    });
    let mut worker = Worker::new(e.clone());
    worker.agents = Some(config.to_str().unwrap().into());
    worker.advance(&r.id).await.unwrap();
    worker.advance(&r.id).await.unwrap();
    worker.advance(&r.id).await.unwrap();
    assert!(e
        .store
        .borrow()
        .report_run(&r.id)
        .unwrap()
        .agent
        .unwrap()
        .session_id
        .is_none());
    assert!(submitted.lock().unwrap().is_some());
    drop(worker);
    drop(e);
    clock.set(7000);
    let s = Store::open(&db).unwrap();
    s.report_recover().unwrap();
    let e = Engine::with_clock(s, {
        let c = clock.clone();
        Rc::new(move || c.get())
    });
    let mut worker = Worker::new(e.clone());
    worker.agents = Some(config.to_str().unwrap().into());
    worker.advance(&r.id).await.unwrap();
    worker.advance(&r.id).await.unwrap();
    let done = e.store.borrow().report_run(&r.id).unwrap();
    assert_eq!(done.status, "complete");
    assert_eq!(
        done.content.unwrap().summary,
        "Agent found all services healthy"
    );
    assert_eq!(done.outputs["analysis"].value["session_id"], "session-1");
    assert!(!serde_json::to_string(&done.outputs)
        .unwrap()
        .contains("fixture-credential"));
    server.verify().await;
    drop(worker);
    drop(e);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn required_agent_failure_and_delivery_recovery_are_explicit() {
    let mut s = Store::open(":memory:").unwrap();
    let mut d = definition();
    d.steps=vec![serde_json::from_value(json!({"id":"analysis","kind":"simple_agents","provider":"missing","instructions":"Report","inputs":[]})).unwrap()];
    s.report_save(&d, 0, 1000).unwrap();
    let mut r = s.report_start("morning", "operator", false, 1000).unwrap();
    let request: Value = serde_json::from_str(include_str!(
        "../../vendor/simple-agents-client/contracts/v1/examples/observe.json"
    ))
    .unwrap();
    let provider: agent::Provider = serde_json::from_value(json!({
        "origin":"http://127.0.0.1:1234", "caller_id":"talia",
        "token_file":"/nonexistent", "profile_id":request["profile_id"],
        "budget":request["budget"]
    }))
    .unwrap();
    let pending = provider
        .prepare("missing", &r, &d.steps[0], "Report", &[])
        .unwrap();
    let evidence = serde_json::to_value(&pending).unwrap();
    r.agent = Some(pending);
    s.report_put(&r).unwrap();
    let e = Engine::with_clock(s, Rc::new(|| 1000));
    let mut w = Worker::new(e.clone());
    w.agents = None;
    w.advance(&r.id).await.unwrap();
    let mut r = e.store.borrow().report_run(&r.id).unwrap();
    assert_eq!(r.status, "failed");
    assert_eq!(r.outputs["analysis"].status, "failed");
    assert_eq!(r.outputs["analysis"].value["submission"], evidence);
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
