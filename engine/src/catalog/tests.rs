use super::*;
fn put(kind: &str, id: &str, doc: Value) -> Change {
    Change::Put {
        key: Key::new(kind, id),
        document: doc,
        migration: None,
        initial: None,
    }
}
fn delete(kind: &str, id: &str) -> Change {
    Change::Delete {
        key: Key::new(kind, id),
    }
}
fn bundle(s: &Store, changes: Vec<Change>) -> ChangeSet {
    ChangeSet {
        expected_catalog_revision: s.catalog_revision().unwrap(),
        changes,
    }
}
fn ui() -> String {
    "<Dashboard id=\"Root\"><Surface id=\"Body\"><Use id=\"Notice\" definition=\"notice\" params={params.notice}/></Surface></Dashboard>".into()
}
fn dashboard() -> Value {
    json!({"ui":ui(),"view_model":"defineVM({initial:()=>({}),actions:{}});","references":[{"kind":"ui","id":"notice"}],"params":{"notice":{}},"grants":{"reads":[],"writes":[],"runs":[]}})
}
fn notice(text: &str) -> Value {
    json!({"source":format!("<Text id=\"Text\" text=\"{text}\"/>"),"references":[]})
}
fn seed_ui(s: &mut Store) {
    let set = bundle(
        s,
        vec![
            put("dashboard", "one", dashboard()),
            put("dashboard", "two", dashboard()),
            put("ui", "notice", notice("before")),
        ],
    );
    s.catalog_save(&set).unwrap();
}
#[test]
fn shared_packages_and_local_parameters() {
    let mut s = Store::open(":memory:").unwrap();
    seed_ui(&mut s);
    let one = s.dashboard_package("one").unwrap();
    let two = s.dashboard_package("two").unwrap();
    let mut local = dashboard();
    local["params"] = json!({"notice":{"label":"local"}});
    s.catalog_save(&bundle(&s, vec![put("dashboard", "one", local)]))
        .unwrap();
    assert_ne!(s.dashboard_package("one").unwrap(), one);
    assert_eq!(s.dashboard_package("two").unwrap(), two);
    let result = s
        .catalog_save(&bundle(&s, vec![put("ui", "notice", notice("after"))]))
        .unwrap();
    assert_eq!(result.packages.len(), 2);
    for id in ["one", "two"] {
        assert_eq!(
            s.dashboard_package(id).unwrap()["definitions"]["notice"]["props"]["text"],
            "after"
        );
    }
    // An already loaded copy remains a coherent old package.
    assert_eq!(one["definitions"]["notice"]["props"]["text"], "before");
}
#[test]
fn validation_conflicts_and_no_authored_execution() {
    let mut s = Store::open(":memory:").unwrap();
    seed_ui(&mut s);
    let old = s.catalog_snapshot().unwrap();
    let pkg = s.dashboard_package("one").unwrap();
    let mut doc = dashboard();
    doc["view_model"] = json!("throw Error('must not execute during authoring');");
    let set = bundle(&s, vec![put("dashboard", "one", doc)]);
    assert!(s.catalog_validate(&set).valid);
    assert_eq!(s.catalog_revision().unwrap(), old.catalog_revision);
    assert_eq!(s.dashboard_package("one").unwrap(), pkg);
    s.catalog_save(&set).unwrap();
    assert_eq!(s.catalog_save(&set).unwrap_err().code, "conflict");
    let bad = bundle(
        &s,
        vec![put(
            "ui",
            "notice",
            json!({"source":"<Text id=\"x\" text={bad + call()}/>"}),
        )],
    );
    let error = s.catalog_save(&bad).unwrap_err();
    assert_eq!(error.key, Some(Key::new("ui", "notice")));
    assert!(error.line.is_some());
}
#[test]
fn deletion_cycles_and_unused_sources_are_validated() {
    let mut s = Store::open(":memory:").unwrap();
    seed_ui(&mut s);
    let rev = s.catalog_revision().unwrap();
    assert!(s
        .catalog_save(&bundle(&s, vec![delete("ui", "notice")]))
        .is_err());
    assert_eq!(s.catalog_revision().unwrap(), rev);
    assert!(s
        .catalog_save(&bundle(
            &s,
            vec![put(
                "function",
                "unused",
                json!({"source":"()=>","references":[]})
            )]
        ))
        .is_err());
    let cycle = json!({"source":"()=>1","references":[{"kind":"function","id":"cycle"}]});
    assert!(s
        .catalog_save(&bundle(&s, vec![put("function", "cycle", cycle)]))
        .is_err());
    s.catalog_save(&bundle(
        &s,
        vec![
            delete("ui", "notice"),
            delete("dashboard", "two"),
            delete("dashboard", "one"),
        ],
    ))
    .unwrap();
    assert!(s.dashboard_package("one").is_err());
}
#[test]
fn engine_records_share_storage_and_legacy_edits_conflict() {
    let mut s = Store::open(":memory:").unwrap();
    let mut i = crate::store::tests::seed(&mut s);
    seed_ui(&mut s);
    let set = bundle(&s, vec![put("ui", "notice", notice("stale"))]);
    i.value = value::number(19.0);
    i.revision += 1;
    s.commit(1, &i, true, 100).unwrap();
    assert_eq!(s.catalog_revision().unwrap(), set.expected_catalog_revision);
    s.reconfigure(&i.id, i.revision, value::number(5.0))
        .unwrap();
    assert_eq!(s.catalog_save(&set).unwrap_err().code, "conflict");
    let mut v = serde_json::to_value(Variable::from(&i)).unwrap();
    v["params"] = value::number(7.0);
    s.catalog_save(&bundle(&s, vec![put("variable", "metric", v)]))
        .unwrap();
    let after = s.instance("metric").unwrap();
    assert_eq!(after.params, value::number(7.0));
    assert_eq!(after.value, value::number(19.0));
    assert_eq!(after.timestamp, i.timestamp);
    assert!(!s.history("metric", 100).unwrap().is_empty());
}
#[test]
fn final_graph_is_order_independent_and_migration_failure_rolls_back() {
    let mut s = Store::open(":memory:").unwrap();
    crate::store::tests::seed(&mut s);
    seed_ui(&mut s);
    let old = s.instance("metric").unwrap();
    let old_rev = s.catalog_revision().unwrap();
    let old_pkg = s.dashboard_package("one").unwrap();
    let mut d = s.definition("stored").unwrap();
    d.version += 1;
    d.state_schema = "number".into();
    let mut change = put(
        "variable_definition",
        "stored",
        serde_json::to_value(&d).unwrap(),
    );
    if let Change::Put { migration, .. } = &mut change {
        *migration = Some("()=>{throw Error('bad migration')}".into());
    }
    assert!(s
        .catalog_save(&bundle(
            &s,
            vec![put("ui", "notice", notice("bad")), change]
        ))
        .is_err());
    assert_eq!(s.instance("metric").unwrap(), old);
    assert_eq!(s.catalog_revision().unwrap(), old_rev);
    assert_eq!(s.dashboard_package("one").unwrap(), old_pkg);
    let mut def = s.definition("stored").unwrap();
    def.id = "newdef".into();
    def.dependencies = vec!["newvar".into()];
    let source = serde_json::to_value(&def).unwrap();
    let newvar = json!({"id":"newvar","definition":"stored","params":value::undefined(),"history_count":2,"history_age_ms":100});
    s.catalog_save(&bundle(
        &s,
        vec![
            put("variable_definition", "newdef", source),
            put("variable", "newvar", newvar),
        ],
    ))
    .unwrap();
    s.catalog_save(&bundle(
        &s,
        vec![
            delete("variable_definition", "newdef"),
            delete("variable", "newvar"),
        ],
    ))
    .unwrap();
}
#[test]
fn mixed_engine_and_ui_failure_is_atomic() {
    let mut s = Store::open(":memory:").unwrap();
    crate::store::tests::seed(&mut s);
    seed_ui(&mut s);
    let c = crate::monitoring::tests::config();
    s.configure_monitoring(&c, 0).unwrap();
    let state = s.monitor_state("x").unwrap();
    let mut d = c.definitions[0].clone();
    d.version += 1;
    d.source = "{async run(ctx){await ctx.publish('metric',9)}}".into();
    let rev = s.catalog_revision().unwrap();
    assert!(s
        .catalog_save(&bundle(
            &s,
            vec![
                put(
                    "monitor_definition",
                    "collect",
                    serde_json::to_value(&d).unwrap()
                ),
                delete("ui", "notice")
            ]
        ))
        .is_err());
    assert_eq!(s.monitoring_config().unwrap(), c);
    assert_eq!(s.monitor_state("x").unwrap().generation, state.generation);
    assert_eq!(s.catalog_revision().unwrap(), rev);
    let result = s
        .catalog_save(&bundle(
            &s,
            vec![
                put(
                    "monitor_definition",
                    "collect",
                    serde_json::to_value(&d).unwrap(),
                ),
                put("ui", "notice", notice("new")),
            ],
        ))
        .unwrap();
    assert_eq!(result.packages.len(), 2);
    assert_eq!(s.monitoring_config().unwrap().definitions[0], d);
    assert!(s.monitor_state("x").unwrap().generation > state.generation);
    // A later legacy monitor edit invalidates authoring snapshots too.
    let rev = s.catalog_revision().unwrap();
    let mut c = s.monitoring_config().unwrap();
    c.version += 1;
    c.instances[0].params = value::number(3.0);
    s.configure_monitoring(&c, c.version - 1).unwrap();
    assert!(s.catalog_revision().unwrap() > rev);
}
#[test]
fn invalid_dashboard_rolls_back_successful_variable_migration() {
    let mut s = Store::open(":memory:").unwrap();
    let original = crate::store::tests::seed(&mut s);
    seed_ui(&mut s);
    let mut d = s.definition("stored").unwrap();
    d.version = 2;
    d.state_schema = "number".into();
    let change = Change::Put {
        key: Key::new("variable_definition", "stored"),
        document: serde_json::to_value(d).unwrap(),
        migration: Some("()=>7".into()),
        initial: None,
    };
    let rev = s.catalog_revision().unwrap();
    assert!(s
        .catalog_save(&bundle(&s, vec![change.clone(), delete("ui", "notice")]))
        .is_err());
    assert_eq!(s.instance("metric").unwrap(), original);
    assert_eq!(s.catalog_revision().unwrap(), rev);
    let set = bundle(&s, vec![change]);
    assert!(s.catalog_validate(&set).valid);
    assert_eq!(s.instance("metric").unwrap(), original);
    s.catalog_save(&set).unwrap();
    assert_eq!(
        s.instance("metric").unwrap().state["value"][1].as_f64(),
        Some(7.0)
    );
    assert_eq!(s.definition("stored").unwrap().version, 2);
}
#[test]
fn packages_and_catalog_survive_reopen() {
    let path = std::env::temp_dir().join(format!("talia-catalog-{}.db", std::process::id()));
    let (revision, package) = {
        let mut s = Store::open(&path).unwrap();
        seed_ui(&mut s);
        (
            s.catalog_revision().unwrap(),
            s.dashboard_package("one").unwrap(),
        )
    };
    let s = Store::open(&path).unwrap();
    assert_eq!(s.catalog_revision().unwrap(), revision);
    assert_eq!(s.dashboard_package("one").unwrap(), package);
    assert_eq!(s.catalog_snapshot().unwrap().records.len(), 3);
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn script_references_link_without_running_code() {
    let mut s = Store::open(":memory:").unwrap();
    let mut doc = dashboard();
    doc["references"] = json!([{"kind":"ui","id":"notice"},{"kind":"vm","id":"child"}]);
    let vm = json!({"source":"{initial:()=>({n:1}),actions:{}}","references":[{"kind":"function","id":"double"}]});
    s.catalog_save(&bundle(
        &s,
        vec![
            put("function", "double", json!({"source":"x=>x*2"})),
            put("vm", "child", vm),
            put("ui", "notice", notice("hello")),
            put("dashboard", "one", doc),
        ],
    ))
    .unwrap();
    let pkg = s.dashboard_package("one").unwrap();
    let code = pkg["viewModel"].as_str().unwrap();
    assert!(code.find("defineFunction").unwrap() < code.find("defineVMReference").unwrap());
    let script = Script::new().unwrap();
    script
        .eval(include_str!("../../../dashboard/shared/vm.js"))
        .unwrap();
    script.eval(code).unwrap();
    script.eval("TaliaVM.start({});").unwrap();
}

#[test]
fn strict_javascript_syntax_and_fragment_locations() {
    let mut s = Store::open(":memory:").unwrap();
    for source in [
        "return 1;",
        "import x from 'file';",
        "with({}){}",
        "defineVM({\n initial:()=>({}),\n broken: @\n});",
    ] {
        let mut d = dashboard();
        d["references"] = json!([]);
        d["ui"]=json!("<Dashboard id=\"D\"><Surface id=\"S\"><Text id=\"T\" text=\"ok\"/></Surface></Dashboard>");
        d["view_model"] = json!(source);
        let e = s
            .catalog_save(&bundle(&s, vec![put("dashboard", "bad", d)]))
            .unwrap_err();
        assert_eq!(e.key, Some(Key::new("dashboard", "bad")));
        if source.contains('@') {
            assert_eq!(e.line, Some(3));
        }
    }
    let e = s
        .catalog_save(&bundle(
            &s,
            vec![put("ui", "bad", json!({"source":"<Wrong id=\"W\"/>"}))],
        ))
        .unwrap_err();
    assert_eq!(e.line, Some(1));
    assert_eq!(e.column, Some(1));
}

#[test]
fn validation_does_not_reserve_old_runtime_state() {
    let mut s = Store::open(":memory:").unwrap();
    let mut i = crate::store::tests::seed(&mut s);
    let mut d = s.definition("stored").unwrap();
    d.version = 2;
    d.state_schema = "number".into();
    let change = Change::Put {
        key: Key::new("variable_definition", "stored"),
        document: serde_json::to_value(d).unwrap(),
        migration: Some("state=>(state??0)+1".into()),
        initial: None,
    };
    let set = bundle(&s, vec![change]);
    assert!(s.catalog_validate(&set).valid);
    i.state = value::number(9.0);
    i.revision += 1;
    s.commit(1, &i, false, 100).unwrap();
    s.catalog_save(&set).unwrap();
    assert_eq!(
        s.instance("metric").unwrap().state["value"][1].as_f64(),
        Some(10.0)
    );
}

#[test]
fn new_variables_initialize_once_and_deleted_ids_fence_old_work() {
    let mut s = Store::open(":memory:").unwrap();
    let old = crate::store::tests::seed(&mut s);
    s.catalog_save(&bundle(&s, vec![delete("variable", "metric")]))
        .unwrap();
    let mut change = put(
        "variable",
        "metric",
        serde_json::to_value(Variable::from(&old)).unwrap(),
    );
    if let Change::Put { initial, .. } = &mut change {
        *initial = Some(Initial {
            state: json!({"version":1,"value":["object",[]]}),
            value: Some(value::number(42.0)),
        });
    }
    s.catalog_save(&bundle(&s, vec![change.clone()])).unwrap();
    let new = s.instance("metric").unwrap();
    assert!(new.generation > old.generation);
    assert_eq!(new.value, value::number(42.0));
    let mut stale = old.clone();
    stale.revision += 1;
    assert!(s.commit(1, &stale, false, 100).is_err());
    assert!(s.catalog_save(&bundle(&s, vec![change])).is_err());
}

#[test]
fn ui_only_save_preserves_unsorted_monitoring_configuration() {
    let mut s = Store::open(":memory:").unwrap();
    crate::store::tests::seed(&mut s);
    let mut c = crate::monitoring::tests::config();
    c.instances.reverse();
    s.configure_monitoring(&c, 0).unwrap();
    seed_ui(&mut s);
    assert_eq!(s.monitoring_config().unwrap(), c);
}

#[test]
fn v4_upgrade_preserves_monitoring_and_variable_state() {
    let path = std::env::temp_dir().join(format!("talia-catalog-v4-{}.db", std::process::id()));
    let mut s = Store::open(&path).unwrap();
    let original = crate::store::tests::seed(&mut s);
    let c = crate::monitoring::tests::config();
    s.configure_monitoring(&c, 0).unwrap();
    let state = serde_json::to_value(s.monitor_state("x").unwrap()).unwrap();
    let triggers: Vec<String> = {
        let mut q = s
            .conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'catalog_%'",
            )
            .unwrap();
        q.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    };
    for name in triggers {
        s.conn
            .execute_batch(&format!("DROP TRIGGER {name}"))
            .unwrap();
    }
    s.conn.execute_batch("DROP TABLE authored_definitions; DROP TABLE dashboard_packages; DELETE FROM metadata WHERE key='catalog_revision'; DROP TABLE alert_entities; DROP TABLE alert_audit; DROP TABLE alert_requests; PRAGMA user_version=4;").unwrap();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.instance("metric").unwrap(), original);
    assert_eq!(s.monitoring_config().unwrap(), c);
    assert_eq!(
        serde_json::to_value(s.monitor_state("x").unwrap()).unwrap(),
        state
    );
    let rev = s.catalog_revision().unwrap();
    s.reconfigure("metric", 1, value::number(4.0)).unwrap();
    assert!(s.catalog_revision().unwrap() > rev);
    seed_ui(&mut s);
    assert!(s.dashboard_package("one").is_ok());
    drop(s);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn final_graph_allows_removing_monitor_and_its_variable_together() {
    let mut s = Store::open(":memory:").unwrap();
    crate::store::tests::seed(&mut s);
    s.configure_monitoring(&crate::monitoring::tests::config(), 0)
        .unwrap();
    let rev = s.catalog_revision().unwrap();
    assert!(s
        .catalog_save(&bundle(&s, vec![delete("variable", "metric")]))
        .is_err());
    assert_eq!(s.catalog_revision().unwrap(), rev);
    s.catalog_save(&bundle(
        &s,
        vec![
            delete("variable_definition", "stored"),
            delete("variable", "metric"),
            delete("monitor_instance", "x"),
        ],
    ))
    .unwrap();
    assert!(s.instance("metric").is_err());
    assert!(s.monitor_state("x").is_err());
    assert!(s.monitor_state("y").is_ok());
}

#[test]
fn authored_package_uses_shared_runtime_and_duplicate_changes_are_rejected() {
    let mut s = Store::open(":memory:").unwrap();
    let doc = json!({"ui":include_str!("../../../dashboard/examples/monitor.ui"),"view_model":include_str!("../../../dashboard/examples/monitor.vm.js"),"references":[{"kind":"ui","id":"notice"}],"params":{"sidebar":false}});
    let fragment = json!({"source":include_str!("../../../dashboard/examples/notice.ui")});
    let set = bundle(
        &s,
        vec![
            put("dashboard", "monitor", doc),
            put("ui", "notice", fragment),
        ],
    );
    s.catalog_save(&set).unwrap();
    let package = s.dashboard_package("monitor").unwrap();
    let runtime = Script::new().unwrap();
    runtime
        .eval(include_str!("../../../dashboard/shared/vm.js"))
        .unwrap();
    runtime
        .eval(package["viewModel"].as_str().unwrap())
        .unwrap();
    runtime.eval("TaliaVM.start({sidebar:false});").unwrap();
    let rev = s.catalog_revision().unwrap();
    assert!(s
        .catalog_save(&bundle(
            &s,
            vec![
                delete("dashboard", "monitor"),
                delete("dashboard", "monitor")
            ]
        ))
        .is_err());
    assert_eq!(s.catalog_revision().unwrap(), rev);
}

#[test]
fn shared_screen_references_resolve_in_the_containing_dashboard() {
    let mut s = Store::open(":memory:").unwrap();
    s.catalog_save(&bundle(
        &s,
        vec![put(
            "ui",
            "destination",
            json!({"source":"<ScreenRef id=\"Link\" screen=\"Overview\"/>"}),
        )],
    ))
    .unwrap();
    let doc = json!({"ui":"<Dashboard id=\"D\"><Screen id=\"Overview\"><Text id=\"T\" text=\"ok\"/></Screen><Surface id=\"S\"><Use id=\"U\" definition=\"destination\" params={params.input}/></Surface></Dashboard>","view_model":"defineVM({initial:()=>({}),actions:{}})","references":[{"kind":"ui","id":"destination"}]});
    s.catalog_save(&bundle(&s, vec![put("dashboard", "one", doc)]))
        .unwrap();
    assert!(s
        .catalog_save(&bundle(
            &s,
            vec![put(
                "ui",
                "destination",
                json!({"source":"<ScreenRef id=\"Link\" screen=\"Missing\"/>"})
            )]
        ))
        .is_err());
}
