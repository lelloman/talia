use super::*;
fn setup() -> Store {
    let mut s = Store::open(":memory:").unwrap();
    s.seed_dashboards().unwrap();
    s.user_bootstrap("issuer#admin").unwrap();
    s.user_seen("issuer#viewer", "Viewer").unwrap();
    s.user_seen("issuer#other", "Other").unwrap();
    s
}
fn access(id: &str) -> Access {
    Access {
        dashboard_id: id.into(),
        owner: "issuer#admin".into(),
        public: false,
        viewers: vec!["issuer#viewer".into()],
        expected_revision: 0,
        request_id: format!("share-{id}"),
    }
}
#[test]
fn default_deny_share_revoke_and_idempotency() {
    let mut s = setup();
    assert_eq!(
        s.user_catalog("issuer#viewer").unwrap()["dashboards"],
        json!([])
    );
    assert!(!s.user_admin("issuer#other").unwrap());
    let mut a = access("monitor");
    let saved = s.dashboard_access_set("issuer#admin", &a).unwrap();
    assert_eq!(s.dashboard_access_set("issuer#admin", &a).unwrap(), saved);
    assert!(s.user_package("issuer#viewer", "monitor").is_ok());
    assert!(s.user_package("issuer#other", "monitor").is_err());
    assert!(s.user_package("issuer#viewer", "monitoring").is_err());
    a.public = true;
    assert_eq!(s.dashboard_access_set("issuer#admin", &a), Err(E::Conflict));
    a.request_id = "publish".into();
    a.expected_revision = 1;
    s.dashboard_access_set("issuer#admin", &a).unwrap();
    assert!(s.user_package("issuer#other", "monitor").is_ok());
    a.public = false;
    a.viewers.clear();
    a.request_id = "unshare".into();
    a.expected_revision = 2;
    s.dashboard_access_set("issuer#admin", &a).unwrap();
    assert!(s.user_package("issuer#viewer", "monitor").is_err());
    assert!(s
        .user_request("issuer#viewer", json!({"op":"share","access":a}))
        .is_err());
    assert!(s
        .user_request(
            "issuer#viewer",
            json!({"op":"role","subject":"issuer#viewer","admin":true})
        )
        .is_err());
    assert!(s
        .user_request("issuer#viewer", json!({"op":"users"}))
        .is_err());
}
#[test]
fn account_defaults_new_clients_overrides_and_revoked_delivery() {
    let mut s = setup();
    for id in ["monitor", "monitoring"] {
        s.dashboard_access_set("issuer#admin", &access(id)).unwrap();
    }
    s.user_default("issuer#viewer", "monitoring").unwrap();
    let owner = "f".repeat(64);
    for ch in ['a', 'b'] {
        let token = ch.to_string().repeat(64);
        s.user_client_request(
            "issuer#viewer",
            &token,
            json!({"op":"register","name":"Web","platform":"web"}),
            0,
        )
        .unwrap();
        let a = s
            .user_client_request(
                "issuer#viewer",
                &token,
                json!({"op":"openSlot","slot":"slot","owner":owner}),
                0,
            )
            .unwrap();
        assert_eq!(a["dashboardId"], "monitoring");
    }
    let t = "a".repeat(64);
    s.user_client_request("issuer#viewer",&t,json!({"op":"select","slot":"slot","owner":owner,"expected":1,"assignment":{"dashboardId":"monitor","params":{},"presentation":{}}}),0).unwrap();
    s.user_default("issuer#viewer", "monitor").unwrap();
    let t2 = "b".repeat(64);
    let a = s
        .user_client_request(
            "issuer#viewer",
            &t2,
            json!({"op":"openSlot","slot":"slot","owner":owner}),
            0,
        )
        .unwrap();
    assert_eq!(a["dashboardId"], "monitoring");
    let d = s
        .user_client_request(
            "issuer#viewer",
            &t,
            json!({"op":"delivery","slot":"slot","owner":owner}),
            0,
        )
        .unwrap();
    assert_eq!(d["package"]["grants"]["writes"], json!([]));
    assert_eq!(d["package"]["grants"]["runs"], json!([]));
    assert!(s.user_client_request("issuer#viewer",&t,json!({"op":"select","slot":"slot","owner":owner,"expected":2,"assignment":{"dashboardId":"monitor","params":{"source":"secret"}}}),0).is_err());
    let mut a = access("monitor");
    a.viewers.clear();
    a.expected_revision = 1;
    a.request_id = "revoke".into();
    s.dashboard_access_set("issuer#admin", &a).unwrap();
    assert!(s
        .user_client_request(
            "issuer#viewer",
            &t,
            json!({"op":"delivery","slot":"slot","owner":owner}),
            0
        )
        .is_err());
    assert!(s.user_default("issuer#viewer", "monitor").is_err());
    let state = s
        .user_client_request(
            "issuer#viewer",
            &t,
            json!({"op":"selectionState","slot":"slot","owner":owner}),
            0,
        )
        .unwrap();
    assert_eq!(state, json!({"revision":2}));
    s.user_client_request("issuer#viewer",&t,json!({"op":"select","slot":"slot","owner":owner,"expected":2,"assignment":{"dashboardId":"monitoring"}}),0).unwrap();
    assert!(s
        .user_client_request(
            "issuer#viewer",
            &t,
            json!({"op":"delivery","slot":"slot","owner":owner}),
            0
        )
        .is_ok());
}
#[test]
fn role_changes_do_not_promote_again_on_bootstrap_and_deleted_dashboards_lose_shares() {
    let mut s = setup();
    s.user_bootstrap("issuer#second").unwrap();
    s.user_request(
        "issuer#admin",
        json!({"op":"role","subject":"issuer#second","admin":false}),
    )
    .unwrap();
    s.user_bootstrap("issuer#second").unwrap();
    assert!(!s.user_admin("issuer#second").unwrap());
    assert!(s
        .user_request(
            "issuer#admin",
            json!({"op":"role","subject":"issuer#admin","admin":false})
        )
        .is_err());
    s.dashboard_access_set("issuer#admin", &access("monitor"))
        .unwrap();
    s.user_default("issuer#viewer", "monitor").unwrap();
    s.conn
        .execute("DELETE FROM dashboard_packages WHERE id='monitor'", [])
        .unwrap();
    assert!(!s.user_can_open("issuer#viewer", "monitor").unwrap());
    assert!(s.user_catalog("issuer#viewer").unwrap()["defaultDashboard"].is_null());
}
#[test]
fn policies_defaults_and_roles_survive_reopening() {
    let path = std::env::temp_dir().join(format!("talia-users-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let mut s = Store::open(&path).unwrap();
        s.seed_dashboards().unwrap();
        s.user_bootstrap("issuer#admin").unwrap();
        s.user_seen("issuer#viewer", "Viewer").unwrap();
        s.dashboard_access_set("issuer#admin", &access("monitor"))
            .unwrap();
        s.user_default("issuer#viewer", "monitor").unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert!(s.user_admin("issuer#admin").unwrap());
    assert_eq!(
        s.user_catalog("issuer#viewer").unwrap()["defaultDashboard"],
        "monitor"
    );
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn scoped_agent_cannot_publish_but_global_operator_can() {
    use crate::authority::{Action, Family, Grant, Scope};
    let mut s = setup();
    for (id, scope) in [
        (
            "scoped",
            Scope::Definition {
                definition_kind: "dashboard".into(),
                id: Some("monitor".into()),
            },
        ),
        ("operator", Scope::All),
    ] {
        s.agent_policy_set(
            id,
            0,
            true,
            &[Grant {
                family: Family::Authoring,
                actions: [Action::Save].into_iter().collect(),
                scope,
            }],
        )
        .unwrap();
        let token = s.agent_credential_issue(id).unwrap();
        let session = s.agent_authenticate(&token).unwrap();
        let r = crate::mcp::dispatch(
            &mut s,
            &session,
            crate::mcp::Request {
                name: "dashboard_access_set".into(),
                arguments: serde_json::to_value(access("monitor")).unwrap(),
            },
            0,
        );
        assert_eq!(r.is_ok(), id == "operator");
    }
}
