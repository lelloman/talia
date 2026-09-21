use super::*;
use crate::{
    authority::{Action, Family, Grant, Scope},
    clients::Report,
    watches::Watches,
};
fn setup() -> (Live, Session, Address) {
    let pipelines = crate::pipelines::tests::fixture("{run(){return 1}}");
    let engine = pipelines.engine.clone();
    let agent = AgentEngine::new(engine.clone(), pipelines.clone(), Watches::new(pipelines));
    let live = Live::new(engine.clone(), agent);
    let mut store = engine.store.borrow_mut();
    let grants = vec![
        Grant {
            family: Family::Live,
            actions: [
                Action::List,
                Action::Inspect,
                Action::Execute,
                Action::Reload,
            ]
            .into(),
            scope: Scope::All,
        },
        Grant {
            family: Family::Engine,
            actions: [Action::Read, Action::Write].into(),
            scope: Scope::All,
        },
    ];
    store.agent_policy_set("agent", 0, true, &grants).unwrap();
    let token = store.agent_credential_issue("agent").unwrap();
    let session = store.agent_authenticate(&token).unwrap();
    store
        .client_register(&"a".repeat(64), "Test", "web")
        .unwrap();
    let host = store.client_authenticate(&"a".repeat(64)).unwrap();
    let report = Report {
        dashboard_id: "monitor".into(),
        package_revision: "rev".into(),
        lifecycle: "active".into(),
        foreground: true,
        dirty: false,
        edit_revision: 0,
        update_available: false,
        assignment_revision: None,
        cached: false,
    };
    store
        .client_connect(
            &host,
            "tab",
            &"b".repeat(64),
            "instance",
            None,
            1,
            report,
            engine.now(),
        )
        .unwrap();
    (
        live,
        session,
        Address {
            client_id: host.id().into(),
            slot_id: "tab".into(),
            live_instance_id: "instance".into(),
        },
    )
}
fn request(a: &Address, id: &str) -> Request {
    Request {
        name: "live_execute".into(),
        arguments: json!({"target":a,"expectedEditRevision":0,"source":"PRIVATE_SOURCE","requestId":id,"timeoutMs":500}),
    }
}
fn body(op: &str, id: Option<&str>) -> Value {
    json!({"op":op,"slot":"tab","owner":"b".repeat(64),"live":"instance","epoch":1,"commandId":id})
}
async fn claim(l: &Live) -> String {
    loop {
        let v = l
            .host(&"a".repeat(64), body("livePoll", None))
            .await
            .unwrap();
        if let Some(id) = v["commands"][0]["id"].as_str() {
            return id.into();
        }
        tokio::task::yield_now().await;
    }
}
async fn begin(l: &Live, id: &str) {
    let mut b = body("liveBegin", Some(id));
    b["report"] = json!({"liveInstanceId":"instance","editRevision":0,"foreground":true,"lifecycle":"active","dirty":false});
    b["grants"] = json!({"reads":["metric"],"writes":["metric"],"runs":[]});
    l.host(&"a".repeat(64), b).await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn cancelled_before_dispatch_and_timed_out_commands_cannot_arrive_later() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (l, s, a) = setup();
            l.cancel(&s, &"c".repeat(64), Some("early")).unwrap();
            assert_eq!(
                l.execute(&s, &"c".repeat(64), "early", request(&a, "early"))
                    .await
                    .unwrap_err(),
                Error::Cancelled
            );
            let mut r = request(&a, "timeout");
            r.arguments["timeoutMs"] = json!(100);
            let v = l.execute(&s, &"c".repeat(64), "late", r).await.unwrap();
            assert_eq!(v["audit"]["status"], "failed");
            assert_eq!(v["error"], "timed_out");
            assert!(l
                .host(&"a".repeat(64), body("livePoll", None))
                .await
                .unwrap()["commands"]
                .as_array()
                .unwrap()
                .is_empty());
            let duplicate = l
                .execute(&s, &"c".repeat(64), "retry", request(&a, "timeout"))
                .await
                .unwrap();
            assert_eq!(duplicate["error"], "conflict");
            assert!(l.pending.borrow().is_empty());
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn connection_ownership_cancellation_and_no_effect_replay() {
    tokio::task::LocalSet::new().run_until(async{
  let(l,s,a)=setup();let worker=l.clone();let session=s.clone();let task=tokio::task::spawn_local(async move{worker.execute(&session,&"c".repeat(64),"call",request(&a,"edit")).await});
  let id=claim(&l).await;let mut forged=body("liveBegin",Some(&id));forged["owner"]=json!("d".repeat(64));assert_eq!(l.host(&"a".repeat(64),forged).await.unwrap_err(),Error::Forbidden);begin(&l,&id).await;
  let mut effect=body("liveEffect",Some(&id));effect["effect"]=json!({"op":"write","id":"metric","sequence":1,"value":crate::value::from_json(&json!(7))});l.host(&"a".repeat(64),effect.clone()).await.unwrap();assert_eq!(l.host(&"a".repeat(64),effect).await.unwrap_err(),Error::Conflict);
  l.cancel(&s,&"c".repeat(64),Some("call")).unwrap();assert_eq!(l.host(&"a".repeat(64),body("liveCheck",Some(&id))).await.unwrap_err(),Error::Cancelled);let out=task.await.unwrap().unwrap();assert_eq!(out["audit"]["status"],"unknown");assert_eq!(l.engine.store.borrow().instance("metric").unwrap().value,crate::value::from_json(&json!(7)));
  let text=out.to_string();assert!(!text.contains("PRIVATE_SOURCE"));assert!(l.pending.borrow().is_empty());
 }).await;
}
#[tokio::test(flavor = "current_thread")]
async fn revocation_fences_effects_and_failed_host_reply_is_durable() {
    tokio::task::LocalSet::new().run_until(async{
  let(l,s,a)=setup();let worker=l.clone();let session=s.clone();let task=tokio::task::spawn_local(async move{worker.execute(&session,&"c".repeat(64),"call",request(&a,"edit")).await});let id=claim(&l).await;begin(&l,&id).await;
  l.engine.store.borrow_mut().agent_policy_set("agent",1,false,&[]).unwrap();let mut effect=body("liveEffect",Some(&id));effect["effect"]=json!({"op":"write","id":"metric","sequence":1,"value":crate::value::from_json(&json!(99))});assert!(l.host(&"a".repeat(64),effect).await.is_err());assert!(task.await.unwrap().is_err());
  assert_ne!(l.engine.store.borrow().instance("metric").unwrap().value,crate::value::from_json(&json!(99)));
 }).await;
}
#[tokio::test(flavor = "current_thread")]
async fn audit_failure_prevents_dispatch_and_host_rejection_finishes_without_begin() {
    tokio::task::LocalSet::new().run_until(async{
  let(l,s,a)=setup();l.engine.store.borrow().conn.execute_batch("CREATE TRIGGER reject_audit BEFORE INSERT ON agent_audit BEGIN SELECT RAISE(FAIL,'test'); END;").unwrap();assert!(l.execute(&s,&"c".repeat(64),"call",request(&a,"failure")).await.is_err());assert!(l.pending.borrow().is_empty());l.engine.store.borrow().conn.execute_batch("DROP TRIGGER reject_audit").unwrap();
  let worker=l.clone();let session=s.clone();let task=tokio::task::spawn_local(async move{worker.execute(&session,&"c".repeat(64),"call",request(&a,"rejected")).await});let id=claim(&l).await;let mut finish=body("liveFinish",Some(&id));finish["value"]=json!({"error":"conflict","editRevision":2});l.host(&"a".repeat(64),finish).await.unwrap();let out=task.await.unwrap().unwrap();assert_eq!(out["audit"]["status"],"failed");assert_eq!(out["error"],"conflict");assert!(l.pending.borrow().is_empty());
 }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn completion_persistence_failure_keeps_prior_effect_unknown_without_replay() {
    tokio::task::LocalSet::new().run_until(async{
  let(l,s,a)=setup();let worker=l.clone();let session=s.clone();let address=a.clone();let task=tokio::task::spawn_local(async move{worker.execute(&session,&"c".repeat(64),"call",request(&address,"unresolved")).await});let id=claim(&l).await;begin(&l,&id).await;
  let mut effect=body("liveEffect",Some(&id));effect["effect"]=json!({"op":"write","id":"metric","sequence":1,"value":crate::value::from_json(&json!(7))});l.host(&"a".repeat(64),effect).await.unwrap();
  l.engine.store.borrow().conn.execute_batch("CREATE TRIGGER fail_outcome BEFORE INSERT ON live_outcomes BEGIN SELECT RAISE(FAIL,'test'); END;").unwrap();let mut finish=body("liveFinish",Some(&id));finish["value"]=json!({"editRevision":1,"liveInstanceId":"instance","packageRevision":"rev"});l.host(&"a".repeat(64),finish).await.unwrap();assert_eq!(task.await.unwrap().unwrap_err(),Error::StorageError);
  assert_eq!(l.engine.store.borrow().agent_status(&s,"unresolved").unwrap().status,Status::Running);l.engine.store.borrow_mut().agent_recover(l.engine.now()).unwrap();assert_eq!(l.engine.store.borrow().agent_status(&s,"unresolved").unwrap().status,Status::Unknown);
  l.execute(&s,&"c".repeat(64),"retry",request(&a,"unresolved")).await.unwrap();assert!(l.pending.borrow().is_empty());assert_eq!(l.engine.store.borrow().instance("metric").unwrap().value,crate::value::from_json(&json!(7)));
  for n in 0..257 {l.cancel(&s,&"c".repeat(64),Some(&format!("cancel-{n}"))).unwrap();}
  assert_eq!(l.execute(&s,&"c".repeat(64),"new",request(&a,"new")).await.unwrap_err(),Error::LimitExceeded);
 }).await;
}
