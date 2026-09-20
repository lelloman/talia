use super::*;
fn grant(family: Family, actions: &[Action], scope: Scope) -> Grant {
    Grant {
        family,
        actions: actions.iter().copied().collect(),
        scope,
    }
}
fn resource(id: &str) -> Target {
    Target::Resource { id: id.into() }
}
fn live() -> Target {
    Target::Live {
        client_id: "browser".into(),
        slot_id: "tab1".into(),
        instance_id: "vm1".into(),
    }
}
fn agent(s: &mut Store, id: &str, grants: &[Grant]) -> (String, Session) {
    s.agent_policy_set(id, 0, true, grants).unwrap();
    let token = s.agent_credential_issue(id).unwrap();
    let session = s.agent_authenticate(&token).unwrap();
    (token, session)
}
fn author() -> Grant {
    grant(
        Family::Authoring,
        &[
            Action::Read,
            Action::List,
            Action::Validate,
            Action::Save,
            Action::Audit,
        ],
        Scope::All,
    )
}
fn engine() -> Grant {
    grant(
        Family::Engine,
        &[Action::Read, Action::Write, Action::Run, Action::Audit],
        Scope::All,
    )
}
fn controller() -> Grant {
    grant(
        Family::Live,
        &[
            Action::Inspect,
            Action::Execute,
            Action::Reload,
            Action::Audit,
        ],
        Scope::All,
    )
}
fn change(s: &Store, id: &str, grants: Value) -> catalog::ChangeSet {
    catalog::ChangeSet {
        expected_catalog_revision: s.catalog_revision().unwrap(),
        changes: vec![catalog::Change::Put {
            key: catalog::Key::new("dashboard", id),
            document: json!({"ui":"<Dashboard id=\"D\"><Surface id=\"S\"><Text id=\"T\" text=\"ok\"/></Surface></Dashboard>","view_model":"defineVM({initial:()=>({}),actions:{}})","grants":grants}),
            migration: None,
            initial: None,
        }],
    }
}
fn empty_grants() -> Value {
    json!({"reads":[],"writes":[],"runs":[]})
}
fn permit(s: &mut Store, session: &Session, id: &str, op: Operation, target: Target) -> Permit {
    s.agent_admit(session, id, op, &[target], &json!({}), &[], 100)
        .unwrap()
        .permit
        .unwrap()
}
#[test]
fn credentials_and_families_are_independent() {
    let mut s = Store::open(":memory:").unwrap();
    let (token, session) = agent(&mut s, "writer", &[author()]);
    assert_eq!(session.principal(), "writer");
    assert!(s.agent_authenticate("writer").is_err());
    assert!(s
        .agent_require(
            &session,
            Operation::DefinitionsSave,
            &Target::Definition {
                key: catalog::Key::new("ui", "one")
            }
        )
        .is_ok());
    assert_eq!(
        s.agent_require(&session, Operation::EngineWrite, &resource("value")),
        Err(ErrorCode::Forbidden)
    );
    assert_eq!(
        s.agent_require(&session, Operation::LiveExecute, &live()),
        Err(ErrorCode::Forbidden)
    );
    let stored: String = s
        .conn
        .query_row("SELECT digest FROM agent_credentials", [], |r| r.get(0))
        .unwrap();
    assert_ne!(stored, token);
    assert!(!stored.contains("writer"));
    let (_, no_grants) = agent(&mut s, "nobody", &[]);
    assert!(matches!(
        s.agent_catalog_snapshot(&no_grants),
        Err(ErrorCode::Forbidden)
    ));
    s.agent_credential_revoke(&token).unwrap();
    assert!(s.agent_authenticate(&token).is_err());
    assert_eq!(
        s.agent_require(
            &session,
            Operation::DefinitionsSave,
            &Target::Definition {
                key: catalog::Key::new("ui", "one")
            }
        ),
        Err(ErrorCode::Unauthenticated)
    );
}
#[test]
fn exact_scopes_and_operation_target_types() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(
        &mut s,
        "scoped",
        &[
            grant(
                Family::Engine,
                &[Action::Read],
                Scope::Resource { id: "cpu".into() },
            ),
            grant(
                Family::Live,
                &[Action::Execute],
                Scope::Client {
                    client_id: "browser".into(),
                    slot_id: Some("tab1".into()),
                    instance_id: Some("vm1".into()),
                },
            ),
        ],
    );
    assert!(s
        .agent_require(&session, Operation::EngineRead, &resource("cpu"))
        .is_ok());
    assert_eq!(
        s.agent_require(&session, Operation::EngineRead, &resource("cpu.extra")),
        Err(ErrorCode::Forbidden)
    );
    assert_eq!(
        s.agent_require(&session, Operation::EngineWrite, &resource("cpu")),
        Err(ErrorCode::Forbidden)
    );
    assert!(s
        .agent_require(&session, Operation::LiveExecute, &live())
        .is_ok());
    let other = Target::Live {
        client_id: "browser".into(),
        slot_id: "tab2".into(),
        instance_id: "vm1".into(),
    };
    assert_eq!(
        s.agent_require(&session, Operation::LiveExecute, &other),
        Err(ErrorCode::Forbidden)
    );
    assert_eq!(
        s.agent_require(&session, Operation::EngineRead, &live()),
        Err(ErrorCode::InvalidInput)
    );
    assert!(s
        .agent_policy_set(
            "bad",
            0,
            true,
            &[grant(Family::Engine, &[Action::Save], Scope::All)]
        )
        .is_err());
}
#[test]
fn admissions_denials_deduplication_and_principal_isolation() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, writer) = agent(&mut s, "writer", &[engine()]);
    let (_, reader) = agent(
        &mut s,
        "reader",
        &[grant(Family::Engine, &[Action::Read], Scope::All)],
    );
    let denied = s
        .agent_admit(
            &reader,
            "write",
            Operation::EngineWrite,
            &[resource("value")],
            &json!({"secret":"not logged"}),
            &[],
            10,
        )
        .unwrap();
    assert!(denied.permit.is_none());
    assert_eq!(denied.record.error, Some(ErrorCode::Forbidden));
    assert_eq!(
        s.agent_status(&reader, "write").unwrap().status,
        Status::Failed
    );
    let p = permit(
        &mut s,
        &writer,
        "same",
        Operation::EngineWrite,
        resource("value"),
    );
    s.agent_start(&p, 101).unwrap();
    assert_eq!(s.agent_start(&p, 102), Err(ErrorCode::Conflict));
    let duplicate = s
        .agent_admit(
            &writer,
            "same",
            Operation::EngineWrite,
            &[resource("value")],
            &json!({}),
            &[],
            110,
        )
        .unwrap();
    assert!(duplicate.permit.is_none());
    assert_eq!(duplicate.record.status, Status::Running);
    let conflict = s
        .agent_admit(
            &writer,
            "same",
            Operation::EngineWrite,
            &[resource("value")],
            &json!({"different":true}),
            &[],
            111,
        )
        .unwrap();
    assert!(conflict.permit.is_none());
    assert_eq!(conflict.record.error, Some(ErrorCode::Conflict));
    assert_eq!(s.agent_status(&reader, "same"), Err(ErrorCode::NotFound));
    s.agent_finish(&p, Status::Complete, None, &[], 120)
        .unwrap();
    assert!(s.agent_check_permit(&p).is_err());
    let (_, writer2) = agent(&mut s, "writer2", &[engine()]);
    assert!(s
        .agent_admit(
            &writer2,
            "same",
            Operation::EngineWrite,
            &[resource("value")],
            &json!({}),
            &[],
            120
        )
        .unwrap()
        .permit
        .is_some());
    assert_eq!(
        s.agent_audit_list(&writer, 0, 100).unwrap().records.len(),
        4
    );
}
#[test]
fn revocation_is_checked_before_start_and_after_await() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(&mut s, "agent", &[engine()]);
    let p = permit(
        &mut s,
        &session,
        "a",
        Operation::EngineWrite,
        resource("value"),
    );
    s.agent_policy_set("agent", 1, true, &[]).unwrap();
    assert_eq!(s.agent_start(&p, 102), Err(ErrorCode::Forbidden));
    s.agent_policy_set("agent", 2, true, &[engine()]).unwrap();
    let p = permit(
        &mut s,
        &session,
        "b",
        Operation::EngineWrite,
        resource("value"),
    );
    s.agent_start(&p, 103).unwrap();
    s.agent_policy_set("agent", 3, false, &[engine()]).unwrap();
    assert_eq!(s.agent_check_permit(&p), Err(ErrorCode::Unauthenticated));
    // A known prior effect is recorded even though further effects are forbidden.
    assert_eq!(
        s.agent_finish(&p, Status::Complete, None, &[], 104)
            .unwrap()
            .status,
        Status::Complete
    );
}
#[test]
fn live_effects_require_host_and_agent_grants_and_active_invocation() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(&mut s, "agent", &[controller()]);
    let p = permit(&mut s, &session, "live", Operation::LiveExecute, live());
    s.agent_start(&p, 101).unwrap();
    let host = HostGrants {
        reads: vec!["cpu".into()],
        writes: vec!["value".into()],
        runs: vec![],
    };
    assert_eq!(
        s.agent_live_effect(&p, Operation::EngineWrite, "value", &host),
        Err(ErrorCode::Forbidden)
    );
    s.agent_policy_set("agent", 1, true, &[controller(), engine()])
        .unwrap();
    assert!(s
        .agent_live_effect(&p, Operation::EngineWrite, "value", &host)
        .is_ok());
    assert_eq!(
        s.agent_live_effect(&p, Operation::EngineWrite, "other", &host),
        Err(ErrorCode::Forbidden)
    );
    s.agent_finish(&p, Status::Cancelled, Some(ErrorCode::Cancelled), &[], 102)
        .unwrap();
    assert_eq!(
        s.agent_live_effect(&p, Operation::EngineWrite, "value", &host),
        Err(ErrorCode::Cancelled)
    );
}
#[test]
fn catalog_success_and_audit_commit_together_with_ceiling() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(&mut s, "author", &[author()]);
    let set = change(&s, "one", json!({"reads":["cpu"],"writes":[],"runs":[]}));
    let original = s.catalog_revision().unwrap();
    let rejected = s.agent_catalog_save(&session, "denied", &set, 10).unwrap();
    assert_eq!(rejected.audit.error, Some(ErrorCode::Forbidden));
    assert_eq!(s.catalog_revision().unwrap(), original);
    assert!(s.dashboard_package("one").is_err());
    s.agent_ceiling_set(&[grant(
        Family::Engine,
        &[Action::Read],
        Scope::Resource { id: "cpu".into() },
    )])
    .unwrap();
    let result = s.agent_catalog_save(&session, "saved", &set, 11).unwrap();
    assert_eq!(result.audit.status, Status::Complete);
    assert!(result.receipt.is_some());
    assert!(result
        .audit
        .after
        .iter()
        .all(|r| r.revision == s.catalog_revision().unwrap().to_string()));
    let rev = s.catalog_revision().unwrap();
    let duplicate = s.agent_catalog_save(&session, "saved", &set, 12).unwrap();
    assert_eq!(duplicate.audit.id, result.audit.id);
    assert!(duplicate.receipt.is_none());
    assert_eq!(s.catalog_revision().unwrap(), rev);
}
#[test]
fn failed_audit_persistence_never_commits_catalog_effect() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(&mut s, "author", &[author()]);
    let set = change(&s, "one", empty_grants());
    let original = s.catalog_revision().unwrap();
    s.conn.execute_batch("CREATE TRIGGER block_success BEFORE UPDATE ON agent_audit WHEN new.status='\"complete\"' BEGIN SELECT RAISE(ABORT,'private database detail'); END;").unwrap();
    let result = s.agent_catalog_save(&session, "save", &set, 10).unwrap();
    assert_eq!(result.audit.error, Some(ErrorCode::StorageError));
    assert_eq!(result.audit.status, Status::Failed);
    assert_eq!(s.catalog_revision().unwrap(), original);
    assert!(s.dashboard_package("one").is_err());
    s.conn.execute_batch("CREATE TRIGGER block_admission BEFORE INSERT ON agent_audit BEGIN SELECT RAISE(ABORT,'no room'); END;").unwrap();
    assert!(matches!(
        s.agent_catalog_save(&session, "save2", &set, 12),
        Err(ErrorCode::StorageError)
    ));
    assert_eq!(s.catalog_revision().unwrap(), original);
}
#[test]
fn audit_failure_after_external_effect_leaves_unknown_on_recovery() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(&mut s, "agent", &[engine()]);
    let p = permit(
        &mut s,
        &session,
        "effect",
        Operation::EngineWrite,
        resource("value"),
    );
    s.agent_start(&p, 101).unwrap();
    let mut value = crate::store::tests::seed(&mut s);
    value.revision += 1;
    value.value = crate::value::number(99.0);
    s.commit(1, &value, true, 102).unwrap();
    s.conn.execute_batch("CREATE TRIGGER block_finish BEFORE UPDATE ON agent_audit BEGIN SELECT RAISE(ABORT,'disk unavailable'); END;").unwrap();
    assert_eq!(
        s.agent_finish(&p, Status::Complete, None, &[], 103),
        Err(ErrorCode::StorageError)
    );
    assert_eq!(
        s.agent_status(&session, "effect").unwrap().status,
        Status::Running
    );
    s.conn.execute_batch("DROP TRIGGER block_finish;").unwrap();
    s.agent_recover(110).unwrap();
    assert_eq!(
        s.agent_status(&session, "effect").unwrap().status,
        Status::Unknown
    );
    assert_eq!(
        s.instance("metric").unwrap().value,
        crate::value::number(99.0)
    );
    assert!(s
        .agent_admit(
            &session,
            "effect",
            Operation::EngineWrite,
            &[resource("value")],
            &json!({}),
            &[],
            120
        )
        .unwrap()
        .permit
        .is_none());
}
#[test]
fn sensitive_arguments_and_migration_errors_are_not_audited() {
    let mut s = Store::open(":memory:").unwrap();
    crate::store::tests::seed(&mut s);
    let (token, session) = agent(&mut s, "agent", &[author(), engine(), controller()]);
    let secret = "resolved-secret-should-not-appear";
    let a = s
        .agent_admit(
            &session,
            "live",
            Operation::LiveExecute,
            &[live()],
            &json!({"source":format!("throw Error('{secret}')"),"token":token}),
            &[],
            1,
        )
        .unwrap();
    let p = a.permit.unwrap();
    s.agent_start(&p, 2).unwrap();
    s.agent_finish(&p, Status::Failed, Some(ErrorCode::InternalError), &[], 3)
        .unwrap();
    let mut d = s.definition("stored").unwrap();
    d.version += 1;
    let set = catalog::ChangeSet {
        expected_catalog_revision: s.catalog_revision().unwrap(),
        changes: vec![catalog::Change::Put {
            key: catalog::Key::new("variable_definition", "stored"),
            document: serde_json::to_value(d).unwrap(),
            migration: Some(format!("()=>{{throw Error('{secret}')}}")),
            initial: None,
        }],
    };
    let validation = s.agent_catalog_validate(&session, &set).unwrap();
    assert!(!validation.valid);
    assert!(!serde_json::to_string(&validation).unwrap().contains(secret));
    let result = s.agent_catalog_save(&session, "save", &set, 4).unwrap();
    assert_eq!(result.audit.error, Some(ErrorCode::ValidationFailed));
    let body = serde_json::to_string(&s.agent_audit_list(&session, 0, 100).unwrap()).unwrap();
    assert!(!body.contains(secret));
    assert!(!body.contains(&token));
    assert!(!body.contains("throw Error"));
    let signatures: Vec<String> = {
        let mut q = s
            .conn
            .prepare("SELECT signature FROM agent_requests")
            .unwrap();
        q.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    };
    assert!(signatures.iter().all(|v| v.len() == 64));
}
#[test]
fn audit_read_scope_and_catalog_discovery_are_filtered() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, admin) = agent(&mut s, "admin", &[author(), engine(), controller()]);
    let one = change(&s, "one", empty_grants());
    s.agent_catalog_save(&admin, "one", &one, 1).unwrap();
    let two = change(&s, "two", empty_grants());
    s.agent_catalog_save(&admin, "two", &two, 2).unwrap();
    for id in ["cpu", "disk"] {
        let p = permit(&mut s, &admin, id, Operation::EngineWrite, resource(id));
        s.agent_start(&p, 3).unwrap();
        s.agent_finish(&p, Status::Complete, None, &[], 4).unwrap();
    }
    let (_, reader) = agent(
        &mut s,
        "reader",
        &[
            grant(
                Family::Engine,
                &[Action::Audit],
                Scope::Resource { id: "cpu".into() },
            ),
            grant(
                Family::Authoring,
                &[Action::Read],
                Scope::Definition {
                    definition_kind: "dashboard".into(),
                    id: Some("one".into()),
                },
            ),
        ],
    );
    let page = s.agent_audit_list(&reader, 0, 100).unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].targets, vec![resource("cpu")]);
    let snapshot = s.agent_catalog_snapshot(&reader).unwrap();
    assert_eq!(snapshot.records.len(), 1);
    assert_eq!(snapshot.records[0].key.id, "one");
    let (_, no_audit) = agent(
        &mut s,
        "plain",
        &[grant(Family::Engine, &[Action::Read], Scope::All)],
    );
    assert!(matches!(
        s.agent_audit_list(&no_audit, 0, 10),
        Err(ErrorCode::Forbidden)
    ));
}
#[test]
fn recovery_and_audit_survive_reopen_without_dispatch() {
    let path = std::env::temp_dir().join(format!("talia-authority-{}.db", std::process::id()));
    let token = {
        let mut s = Store::open(&path).unwrap();
        let (token, session) = agent(&mut s, "agent", &[controller()]);
        let _pending = permit(&mut s, &session, "pending", Operation::LiveReload, live());
        let running = permit(&mut s, &session, "running", Operation::LiveExecute, live());
        s.agent_start(&running, 101).unwrap();
        token
    };
    let mut s = Store::open(&path).unwrap();
    let session = s.agent_authenticate(&token).unwrap();
    assert_eq!(s.agent_recover(200).unwrap(), 2);
    assert_eq!(
        s.agent_status(&session, "pending").unwrap().status,
        Status::Failed
    );
    assert_eq!(
        s.agent_status(&session, "running").unwrap().status,
        Status::Unknown
    );
    assert!(s
        .agent_admit(
            &session,
            "running",
            Operation::LiveExecute,
            &[live()],
            &json!({}),
            &[],
            201
        )
        .unwrap()
        .permit
        .is_none());
    assert_eq!(s.agent_audit_list(&session, 0, 1).unwrap().records.len(), 1);
    drop(s);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn unsupported_fields_and_credential_authority_do_not_enter_catalog() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(&mut s, "author", &[author()]);
    let bad = catalog::ChangeSet {
        expected_catalog_revision: s.catalog_revision().unwrap(),
        changes: vec![catalog::Change::Put {
            key: catalog::Key::new("agent_policy", "author"),
            document: json!({"grants":"all"}),
            migration: None,
            initial: None,
        }],
    };
    assert!(matches!(
        s.agent_catalog_save(&session, "escalate", &bad, 1),
        Err(ErrorCode::InvalidInput)
    ));
    assert_eq!(
        s.agent_require(&session, Operation::EngineWrite, &resource("value")),
        Err(ErrorCode::Forbidden)
    );
    assert!(serde_json::from_value::<Grant>(
        json!({"family":"engine","actions":["read"],"scope":{"kind":"all"},"principal":"root"})
    )
    .is_err());
}

#[test]
fn restart_upgrade_preserves_catalog_and_credentials_are_not_sources() {
    let path = std::env::temp_dir().join(format!("talia-authority-v5-{}.db", std::process::id()));
    let mut s = Store::open(&path).unwrap();
    let original = crate::store::tests::seed(&mut s);
    let set = change(&s, "one", empty_grants());
    s.catalog_save(&set).unwrap();
    let package = s.dashboard_package("one").unwrap();
    let revision = s.catalog_revision().unwrap();
    s.conn.execute_batch("DROP TABLE agent_requests; DROP TABLE agent_audit; DROP TABLE agent_credentials; DROP TABLE agent_principals; DROP TABLE agent_security; PRAGMA user_version=5;").unwrap();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.catalog_revision().unwrap(), revision);
    assert_eq!(s.instance("metric").unwrap(), original);
    assert_eq!(s.dashboard_package("one").unwrap(), package);
    let (token, session) = agent(&mut s, "author", &[author()]);
    let serialized = serde_json::to_string(&s.agent_catalog_snapshot(&session).unwrap()).unwrap();
    assert!(!serialized.contains(&token));
    assert!(!serialized.contains("credential_digest"));
    drop(s);
    let s = Store::open(&path).unwrap();
    assert!(s.agent_authenticate(&token).is_ok());
    drop(s);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn changed_request_failures_are_audited_without_overwriting_original_outcomes() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(&mut s, "controller", &[controller()]);
    let original = s
        .agent_admit(
            &session,
            "reload",
            Operation::LiveReload,
            &[live()],
            &json!({"discardDirty":true}),
            &[],
            1,
        )
        .unwrap();
    let p = original.permit.unwrap();
    s.agent_start(&p, 2).unwrap();
    s.agent_finish(&p, Status::Complete, None, &[], 3).unwrap();
    let changed = s
        .agent_admit(
            &session,
            "reload",
            Operation::LiveReload,
            &[live()],
            &json!({"discardDirty":false}),
            &[Revision {
                target: live(),
                revision: "private-revision".into(),
            }],
            4,
        )
        .unwrap();
    assert_eq!(changed.record.error, Some(ErrorCode::Conflict));
    assert!(changed.record.before.is_empty());
    assert!(changed.permit.is_none());
    assert_eq!(
        s.agent_status(&session, "reload").unwrap().status,
        Status::Complete
    );
    let events = s.agent_audit_list(&session, 0, 100).unwrap();
    assert_eq!(events.records.len(), 2);
    assert_eq!(events.records[0].operation, Operation::LiveReload);
}

#[test]
fn permitted_catalog_reads_do_not_grant_audit_access_or_references() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(
        &mut s,
        "author",
        &[grant(
            Family::Authoring,
            &[Action::Read, Action::Save, Action::Validate],
            Scope::Definition {
                definition_kind: "dashboard".into(),
                id: Some("one".into()),
            },
        )],
    );
    let mut set = change(&s, "one", empty_grants());
    if let catalog::Change::Put { document, .. } = &mut set.changes[0] {
        document["references"] = json!([{"kind":"function","id":"private"}]);
    }
    let result = s.agent_catalog_save(&session, "save", &set, 1).unwrap();
    assert_eq!(result.audit.error, Some(ErrorCode::Forbidden));
    assert_eq!(s.catalog_revision().unwrap(), 0);
    assert!(matches!(
        s.agent_audit_list(&session, 0, 10),
        Err(ErrorCode::Forbidden)
    ));
}

#[test]
fn outstanding_dispatch_is_bounded_and_cancelled_permits_stay_retired() {
    let mut s = Store::open(":memory:").unwrap();
    let (_, session) = agent(&mut s, "agent", &[engine()]);
    let mut first = None;
    for n in 0..128 {
        let p = permit(
            &mut s,
            &session,
            &format!("request-{n}"),
            Operation::EngineWrite,
            resource("value"),
        );
        if n == 0 {
            first = Some(p);
        }
    }
    let full = s
        .agent_admit(
            &session,
            "overflow",
            Operation::EngineWrite,
            &[resource("value")],
            &json!({}),
            &[],
            2,
        )
        .unwrap();
    assert_eq!(full.record.error, Some(ErrorCode::LimitExceeded));
    assert!(full.permit.is_none());
    let first = first.unwrap();
    s.agent_finish(
        &first,
        Status::Cancelled,
        Some(ErrorCode::Cancelled),
        &[],
        3,
    )
    .unwrap();
    assert_eq!(s.agent_start(&first, 4), Err(ErrorCode::Conflict));
    assert!(s
        .agent_admit(
            &session,
            "new",
            Operation::EngineWrite,
            &[resource("value")],
            &json!({}),
            &[],
            5
        )
        .unwrap()
        .permit
        .is_some());
}
