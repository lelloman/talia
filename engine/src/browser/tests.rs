use super::*;
use talia_engine::users::Access;
fn setup() -> Service {
    let mut store = Store::open(":memory:").unwrap();
    store.seed_dashboards().unwrap();
    store.user_bootstrap("issuer#admin").unwrap();
    store.user_seen("issuer#viewer", "Viewer").unwrap();
    store
        .define(
            &Definition {
                id: "stored".into(),
                version: 1,
                source: "".into(),
                kind: "stored".into(),
                value_schema: "any".into(),
                state_schema: "any".into(),
                dependencies: vec![],
                read_policy: "shared".into(),
            },
            0,
            None,
        )
        .unwrap();
    for id in ["value", "secret"] {
        store
            .add_instance(&Instance {
                id: id.into(),
                definition: "stored".into(),
                params: value::from_json(&json!({"secret":"private parameters"})),
                state: value::from_json(&json!({"secret":"private state"})),
                value: value::number(12.),
                has_value: true,
                timestamp: 0,
                quality: "good".into(),
                revision: 1,
                generation: 1,
                history_count: 10,
                history_age_ms: 86400000,
            })
            .unwrap();
    }
    store
        .dashboard_access_set(
            "issuer#admin",
            &Access {
                dashboard_id: "monitor".into(),
                owner: "issuer#admin".into(),
                public: false,
                viewers: vec!["issuer#viewer".into()],
                expected_revision: 0,
                request_id: "share".into(),
            },
        )
        .unwrap();
    let engine = Engine::new(store);
    let pipelines = Pipelines::new(engine.clone()).unwrap();
    let watches = Watches::new(pipelines.clone());
    let agent_engine = talia_engine::mcp_engine::AgentEngine::new(
        engine.clone(),
        pipelines.clone(),
        watches.clone(),
    );
    Service {
        live: talia_engine::mcp_live::Live::new(engine.clone(), agent_engine.clone()),
        agent_engine,
        alert_policies: talia_engine::alerts::policy::Policies::new(engine.clone()),
        alert_sender: talia_engine::alerts::providers::Sender::new(engine.clone()),
        reports: talia_engine::reports::worker::Worker::new(engine.clone()),
        engine,
        pipelines,
        watches,
        monitoring_error: Default::default(),
        incarnation: "test".into(),
        clients: Default::default(),
        leases: Default::default(),
    }
}
fn request(s: &Service, op: &str, args: Value) -> Request {
    let (reply, _) = oneshot::channel();
    let p = s
        .engine
        .store
        .borrow()
        .user_package("issuer#viewer", "monitor")
        .unwrap();
    Request {
        http_mcp: false,
        browser_subject: Some("issuer#viewer".into()),
        body: json!({"version":1,"epoch":1,"client":"tab","incarnation":"test","op":op,"args":args,"dashboard":{"id":"monitor","revision":p["revision"]}}),
        credential: None,
        agent: false,
        connection: String::new(),
        call: String::new(),
        reply,
    }
}
#[tokio::test]
async fn viewer_snapshots_subscriptions_and_getters_never_leak_other_resources() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = setup();
            for op in ["hello", "snapshot", "poll", "subscribe"] {
                let r = request(&s, op, json!({"id":"value"}));
                let v = s.browser_execute("issuer#viewer", &r).await.unwrap();
                assert_eq!(v["values"].as_array().unwrap().len(), 1);
                assert_eq!(v["values"][0]["id"], "value");
                assert!(v.get("monitoringError").is_none());
                assert!(!v.to_string().contains("private"));
            }
            let read = s
                .browser_execute("issuer#viewer", &request(&s, "read", json!({"id":"value"})))
                .await
                .unwrap();
            assert!(read.get("state").is_none());
            assert!(read.get("params").is_none());
            assert_eq!(read["value"], value::number(12.));
            for op in ["read", "history", "subscribe"] {
                assert_eq!(
                    s.browser_execute("issuer#viewer", &request(&s, op, json!({"id":"secret"})))
                        .await,
                    Err("forbidden".into())
                );
            }
        })
        .await;
}
#[tokio::test]
async fn viewer_cannot_write_author_operate_or_forge_package() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = setup();
            for op in [
                "write",
                "set",
                "run",
                "define",
                "create",
                "parameters",
                "remove",
                "removeDefinition",
                "configureMonitoring",
                "monitoringConfig",
                "runs",
                "runStatus",
                "cancelRun",
                "resumeWatch",
                "invalidate",
                "status",
            ] {
                assert_eq!(
                    s.browser_execute(
                        "issuer#viewer",
                        &request(&s, op, json!({"id":"value","value":value::number(99.)}))
                    )
                    .await,
                    Err("forbidden".into()),
                    "{op}"
                );
            }
            let mut r = request(&s, "read", json!({"id":"secret"}));
            r.body["dashboard"]["grants"] = json!({"reads":["secret"]});
            assert!(s.browser_execute("issuer#viewer", &r).await.is_err());
            r.body["dashboard"]["id"] = json!("monitoring");
            assert!(s.browser_execute("issuer#viewer", &r).await.is_err());
            r.body["dashboard"] = Value::Null;
            assert!(s.browser_execute("issuer#viewer", &r).await.is_err());
            assert_eq!(
                s.engine.store.borrow().instance("value").unwrap().revision,
                1
            );
            let mut r = request(&s, "hello", json!({}));
            r.credential = Some("a".repeat(64));
            r.body = json!({"op":"liveEffect"});
            assert_eq!(
                s.browser_execute("issuer#viewer", &r).await,
                Err("forbidden".into())
            );
        })
        .await;
}
#[tokio::test]
async fn revoking_share_stops_existing_reads_and_subscriptions() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = setup();
            let r = request(&s, "subscribe", json!({"id":"value"}));
            s.browser_execute("issuer#viewer", &r).await.unwrap();
            let poll = request(&s, "poll", json!({}));
            s.engine
                .store
                .borrow_mut()
                .dashboard_access_set(
                    "issuer#admin",
                    &Access {
                        dashboard_id: "monitor".into(),
                        owner: "issuer#admin".into(),
                        public: false,
                        viewers: vec![],
                        expected_revision: 1,
                        request_id: "revoke".into(),
                    },
                )
                .unwrap();
            assert_eq!(
                s.browser_execute("issuer#viewer", &poll).await,
                Err("forbidden".into())
            );
            assert_eq!(
                s.browser_execute("issuer#viewer", &r).await,
                Err("forbidden".into())
            );
        })
        .await;
}
#[tokio::test]
async fn revocation_during_async_getter_discards_result() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = setup();
            {
                let mut store = s.engine.store.borrow_mut();
                store
                    .define(
                        &Definition {
                            id: "slow".into(),
                            version: 1,
                            source: "{async get(c){await c.sleep(100);return 42}}".into(),
                            kind: "computed".into(),
                            value_schema: "any".into(),
                            state_schema: "any".into(),
                            dependencies: vec![],
                            read_policy: "shared".into(),
                        },
                        0,
                        None,
                    )
                    .unwrap();
                let mut i = store.instance("value").unwrap();
                store.remove_instance("value").unwrap();
                i.definition = "slow".into();
                store.add_instance(&i).unwrap();
            }
            let r = request(&s, "read", json!({"id":"value"}));
            let task = {
                let s = s.clone();
                tokio::task::spawn_local(
                    async move { s.browser_execute("issuer#viewer", &r).await },
                )
            };
            tokio::time::sleep(Duration::from_millis(20)).await;
            s.engine
                .store
                .borrow_mut()
                .dashboard_access_set(
                    "issuer#admin",
                    &Access {
                        dashboard_id: "monitor".into(),
                        owner: "issuer#admin".into(),
                        public: false,
                        viewers: vec![],
                        expected_revision: 1,
                        request_id: "revoke".into(),
                    },
                )
                .unwrap();
            assert_eq!(task.await.unwrap(), Err("forbidden".into()));
        })
        .await;
}
#[tokio::test]
async fn viewer_alerts_and_account_admin_operations_are_denied() {
    tokio::task::LocalSet::new().run_until(async{let s=setup();for op in ["snapshot","history","acknowledge","silence_save","config","device_register"]{let mut r=request(&s,"hello",json!({}));r.agent=true;r.body["name"]=json!(format!("alerts_{op}"));r.body["arguments"]=json!({});assert!(s.browser_execute("issuer#viewer",&r).await.is_err());}
 let mut r=request(&s,"hello",json!({}));r.connection="account".into();r.body=json!({"name":"Viewer","request":{"op":"role","subject":"issuer#viewer","admin":true}});assert!(s.browser_execute("issuer#viewer",&r).await.is_err());
 let mut r=request(&s,"snapshot",json!({}));r.body["dashboard"]=Value::Null;assert_eq!(s.browser_execute("issuer#admin",&r).await.unwrap()["values"].as_array().unwrap().len(),2);
 }).await;
}

// Full-service browser fixture: loopback OIDC is enabled only in this test binary.
#[test]
#[ignore = "launched only by dashboard/tests/access.mjs"]
fn browser_fixture_service(){
 let db=std::env::var("TALIA_TEST_DB").expect("browser fixture database");
 tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(crate::run(vec!["fixture".into(),db,"0".into(),"--seed".into()])).unwrap();
}
