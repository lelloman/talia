use super::*;
use crate::{
    authority::{Action, Family, Grant, Scope},
    store::Definition,
};
fn grant(actions: &[Action], scope: Scope) -> Grant {
    Grant {
        family: Family::Engine,
        actions: actions.iter().copied().collect(),
        scope,
    }
}
fn all() -> Vec<Grant> {
    vec![grant(
        &[
            Action::Read,
            Action::History,
            Action::Subscribe,
            Action::Write,
            Action::Set,
            Action::Run,
            Action::RunStatus,
            Action::CancelRun,
            Action::ResumeWatch,
            Action::Audit,
        ],
        Scope::All,
    )]
}
fn setup(source: &str) -> (AgentEngine, Session, String) {
    let p = crate::pipelines::tests::fixture(source);
    let e = p.engine.clone();
    let w = Watches::new(p.clone());
    let token = {
        let mut s = e.store.borrow_mut();
        s.agent_policy_set("agent", 0, true, &all()).unwrap();
        s.agent_credential_issue("agent").unwrap()
    };
    let session = e.store.borrow().agent_authenticate(&token).unwrap();
    (AgentEngine::new(e, p, w), session, token)
}
async fn call(a: &AgentEngine, s: &Session, name: &str, args: Value) -> Result<Value> {
    a.execute(
        s,
        &"a".repeat(64),
        &random_id().unwrap(),
        Request {
            name: name.into(),
            arguments: args,
        },
    )
    .await
}
fn computed(a: &AgentEngine, source: &str) {
    let mut s = a.engine.store.borrow_mut();
    let d = Definition {
        id: "computed".into(),
        version: 1,
        source: source.into(),
        kind: "computed".into(),
        value_schema: "any".into(),
        state_schema: "any".into(),
        dependencies: vec!["metric".into()],
        read_policy: "shared".into(),
    };
    s.define(&d, 0, None).unwrap();
    let mut i = s.instance("metric").unwrap();
    i.id = "computed".into();
    i.definition = "computed".into();
    i.state = value::from_json(&json!({"private":"secret"}));
    s.add_instance(&i).unwrap();
}
#[tokio::test]
async fn stored_writes_exceptional_samples_history_and_no_replay() {
    tokio::task::LocalSet::new().run_until(async {
 let (a,s,_)=setup("{run(){}}");
 for (n,number) in [f64::NAN,f64::INFINITY,f64::NEG_INFINITY,-0.0].into_iter().enumerate() {
  let i=a.engine.store.borrow().instance("metric").unwrap();let args=json!({"id":"metric","expectedRevision":i.revision,"value":value::number(number),"requestId":format!("w{n}")});
  let first=call(&a,&s,"engine_write",args.clone()).await.unwrap();assert_eq!(first["audit"]["status"],"complete");
  assert_eq!(call(&a,&s,"engine_write",args).await.unwrap()["audit"]["id"],first["audit"]["id"]);
  let read=call(&a,&s,"engine_read",json!({"id":"metric"})).await.unwrap();assert_eq!(read["sample"]["value"],value::number(number));assert_eq!(read["sample"]["revision"],i.revision+1);assert!(read["sample"].get("state").is_none());
 }
 let h=call(&a,&s,"engine_history",json!({"id":"metric","limit":1})).await.unwrap();assert_eq!(h["samples"][0]["value"],value::number(-0.0));assert!(h["nextCursor"].is_string());
 let h2=call(&a,&s,"engine_history",json!({"id":"metric","cursor":h["nextCursor"],"limit":1})).await.unwrap();assert_eq!(h2["samples"][0]["value"],value::number(f64::NEG_INFINITY));
 let failed=call(&a,&s,"engine_write",json!({"id":"metric","value":value::number(99.),"expectedRevision":1,"requestId":"stale"})).await.unwrap();assert_eq!(failed["error"],"conflict");assert_eq!(failed["currentRevision"],5);
}).await;
}
#[tokio::test(start_paused = true)]
async fn subscriptions_are_connection_owned_expire_and_release_demand() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (a, s, _) = setup("{run(){}}");
            computed(&a, "{get(c){return c.read('metric')}}");
            let first = call(&a, &s, "engine_subscribe", json!({"ids":["computed"]}))
                .await
                .unwrap();
            let sub = first["subscriptionId"].clone();
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(10)).await;
            let other = a
                .execute(
                    &s,
                    &"b".repeat(64),
                    "poll",
                    Request {
                        name: "engine_poll".into(),
                        arguments: json!({"subscriptionId":sub}),
                    },
                )
                .await;
            assert_eq!(other, Err(Error::NotFound));
            let changed = call(&a, &s, "engine_poll", json!({"subscriptionId":sub}))
                .await
                .unwrap();
            assert!(!changed["values"].as_array().unwrap().is_empty());
            call(&a, &s, "engine_unsubscribe", json!({"subscriptionId":sub}))
                .await
                .unwrap();
            let runs = a.engine.runs();
            a.engine.changed("metric");
            tokio::task::yield_now().await;
            assert_eq!(a.engine.runs(), runs);
            let lease = call(&a, &s, "engine_subscribe", json!({"ids":["metric"]}))
                .await
                .unwrap();
            tokio::time::advance(LEASE + Duration::from_secs(1)).await;
            a.sweep();
            assert_eq!(
                call(
                    &a,
                    &s,
                    "engine_poll",
                    json!({"subscriptionId":lease["subscriptionId"]})
                )
                .await,
                Err(Error::NotFound)
            );
            let lease = call(&a, &s, "engine_subscribe", json!({"ids":["metric"]}))
                .await
                .unwrap();
            a.close(&s, &"a".repeat(64)).unwrap();
            assert_eq!(
                call(
                    &a,
                    &s,
                    "engine_poll",
                    json!({"subscriptionId":lease["subscriptionId"]})
                )
                .await,
                Err(Error::Cancelled)
            );
        })
        .await;
}
#[tokio::test]
async fn setter_cancellation_keeps_prior_effects_and_skips_later_work() {
    tokio::task::LocalSet::new().run_until(async {
 let (a,s,_)=setup("{run(){}}");computed(&a,"{get(){return 0},async set(c,v){await c.write('metric',7);c.state={committed:true};await c.commit();await c.sleep(100);await c.write('metric',99);return v}}");
 let task={let a=a.clone();let s=s.clone();tokio::task::spawn_local(async move{a.execute(&s,&"a".repeat(64),"setter",Request{name:"engine_set".into(),arguments:json!({"id":"computed","expectedRevision":1,"value":value::number(4.),"requestId":"cancelled-set"})}).await})};
 tokio::time::sleep(Duration::from_millis(20)).await;
 assert_eq!(a.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),Some(7.));
 call(&a,&s,"_cancel_call",json!({"callId":"setter"})).await.unwrap();let out=task.await.unwrap().unwrap();assert_eq!(out["audit"]["status"],"cancelled");
 assert_eq!(a.engine.store.borrow().instance("metric").unwrap().value["value"][1].as_f64(),Some(7.));assert_eq!(a.engine.store.borrow().instance("computed").unwrap().state,value::from_json(&json!({"committed":true})));
 assert_eq!(a.status(&s,"cancelled-set").unwrap()["audit"]["status"],"cancelled");
}).await;
}
#[tokio::test]
async fn permissions_are_rechecked_after_await_and_for_nested_effects() {
    tokio::task::LocalSet::new().run_until(async {
 let (a,s,_)=setup("{run(){}}");computed(&a,"{get(){return 0},async set(c,v){await c.sleep(40);await c.write('metric',99);return v}}");
 let grants=vec![grant(&[Action::Set],Scope::Resource{id:"computed".into()})];a.engine.store.borrow_mut().agent_policy_set("agent",1,true,&grants).unwrap();
 let args=json!({"id":"computed","expectedRevision":1,"value":value::number(4.),"requestId":"nested-denied"});let out=call(&a,&s,"engine_set",args).await.unwrap();assert_eq!(out["error"],"forbidden");assert_eq!(a.engine.store.borrow().instance("metric").unwrap().revision,1);
 a.engine.store.borrow_mut().agent_policy_set("agent",2,true,&all()).unwrap();
 let task={let a=a.clone();let s=s.clone();tokio::task::spawn_local(async move{call(&a,&s,"engine_set",json!({"id":"computed","expectedRevision":1,"value":value::number(4.),"requestId":"revoked"})).await})};
 tokio::time::sleep(Duration::from_millis(10)).await;a.engine.store.borrow_mut().agent_policy_set("agent",3,false,&[]).unwrap();assert_eq!(task.await.unwrap(),Err(Error::Unauthenticated));assert_eq!(a.engine.store.borrow().instance("metric").unwrap().revision,1);
}).await;
}
#[tokio::test]
async fn runs_resolve_ownership_and_admission_survives_lost_response() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (a, s, _) =
                setup("{async run(c){await c.sleep(50);await c.publish('metric',8);return 8}}");
            let args = json!({"id":"x","requestId":"run"});
            let first = call(&a, &s, "engine_run", args.clone()).await.unwrap();
            let id = first["admission"]["run_id"].as_str().unwrap();
            assert_eq!(
                a.status(&s, "run").unwrap()["admission"],
                first["admission"]
            );
            assert_eq!(call(&a, &s, "engine_run", args).await.unwrap(), first);
            let restricted = vec![grant(
                &[Action::RunStatus, Action::CancelRun],
                Scope::Resource { id: "y".into() },
            )];
            a.engine
                .store
                .borrow_mut()
                .agent_policy_set("agent", 1, true, &restricted)
                .unwrap();
            assert_eq!(
                call(&a, &s, "engine_run_status", json!({"runId":id})).await,
                Err(Error::Forbidden)
            );
            let denied = call(
                &a,
                &s,
                "engine_cancel_run",
                json!({"runId":id,"requestId":"cancel"}),
            )
            .await
            .unwrap();
            assert_eq!(denied["error"], "forbidden");
            let run = crate::pipelines::tests::finish(&a.pipelines, id).await;
            assert_eq!(run.status, "complete");
            assert_eq!(
                a.engine.store.borrow().instance("metric").unwrap().revision,
                2
            );
        })
        .await;
}
#[tokio::test]
async fn audit_failures_prevent_admission_and_never_replay_committed_writes() {
    tokio::task::LocalSet::new().run_until(async {
 let (a,s,_)=setup("{run(){}}");let args=json!({"id":"metric","expectedRevision":1,"value":value::number(2.),"requestId":"write"});
 a.engine.store.borrow().conn.execute_batch("CREATE TRIGGER reject_audit BEFORE INSERT ON agent_audit BEGIN SELECT RAISE(FAIL,'fixture'); END;").unwrap();assert_eq!(call(&a,&s,"engine_write",args.clone()).await,Err(Error::StorageError));assert_eq!(a.engine.store.borrow().instance("metric").unwrap().revision,1);
 a.engine.store.borrow().conn.execute_batch("DROP TRIGGER reject_audit; CREATE TRIGGER reject_complete BEFORE UPDATE ON agent_audit WHEN NEW.status='\"complete\"' BEGIN SELECT RAISE(FAIL,'fixture'); END;").unwrap();assert_eq!(call(&a,&s,"engine_write",args.clone()).await,Err(Error::StorageError));assert_eq!(a.engine.store.borrow().instance("metric").unwrap().revision,2);
 a.engine.store.borrow_mut().agent_recover(a.engine.now()).unwrap();assert_eq!(call(&a,&s,"engine_write",args).await.unwrap()["audit"]["status"],"unknown");assert_eq!(a.engine.store.borrow().instance("metric").unwrap().revision,2);
 let before=a.engine.store.borrow().runs().unwrap().len();let r=call(&a,&s,"engine_run",json!({"id":"x","requestId":"failed-admission"})).await.unwrap();assert_eq!(r["error"],"storage_error");assert_eq!(a.engine.store.borrow().runs().unwrap().len(),before);
}).await;
}
