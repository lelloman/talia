use super::*;
fn report() -> Report {
    Report {
        dashboard_id: "monitor".into(),
        package_revision: "v1".into(),
        lifecycle: "active".into(),
        foreground: true,
        dirty: false,
        edit_revision: 0,
        update_available: false,
        assignment_revision: None,
        cached: false,
    }
}
fn setup() -> (Store, Host, String) {
    let mut s = Store::open(":memory:").unwrap();
    let credential = "a".repeat(64);
    s.client_register(&credential, "Desk", "web").unwrap();
    let h = s.client_authenticate(&credential).unwrap();
    (s, h, "b".repeat(64))
}
#[test]
fn credentials_names_and_scopes() {
    let (mut s, h, owner) = setup();
    let a = s
        .client_register(&"a".repeat(64), "Changed", "web")
        .unwrap();
    assert_eq!(a["name"], "Desk");
    assert_eq!(a["clientId"], h.id());
    s.client_rename(&h, "Kitchen").unwrap();
    assert_eq!(s.clients_list(0).unwrap()[0]["name"], "Kitchen");
    assert!(s.client_authenticate(&"c".repeat(64)).is_err());
    s.client_connect(&h, "tab", &owner, "live", None, 1, report(), 0)
        .unwrap();
    assert!(s
        .client_connect(&h, "tab", &"c".repeat(64), "live", None, 2, report(), 1)
        .is_err());
    let text = serde_json::to_string(&s.clients_list(1).unwrap()).unwrap();
    assert!(!text.contains(&owner));
    assert!(!text.contains(&"a".repeat(64)));
}
#[test]
fn separate_tabs_replacement_and_delayed_reports() {
    let (mut s, h, o) = setup();
    s.client_connect(&h, "a", &o, "live-a", None, 1, report(), 0)
        .unwrap();
    let mut other = report();
    other.dashboard_id = "other".into();
    s.client_connect(&h, "b", &o, "live-b", None, 1, other, 0)
        .unwrap();
    s.client_connect(&h, "a", &o, "live-new", Some("live-a"), 2, report(), 1)
        .unwrap();
    assert!(s
        .client_live_target(h.id(), "a", "live-a", true, 2)
        .is_err());
    assert_eq!(
        s.client_live_target(h.id(), "b", "live-b", true, 2)
            .unwrap()
            .report
            .dashboard_id,
        "other"
    );
    assert!(s
        .client_report(&h, "a", &o, "live-a", 1, 100, Some(report()), 2)
        .is_err());
    assert!(s
        .client_connect(&h, "a", &o, "live-a", Some("live-new"), 3, report(), 3)
        .is_err());
}
#[test]
fn lifecycle_lease_and_reconnect() {
    let (mut s, h, o) = setup();
    s.client_connect(&h, "s", &o, "l", None, 1, report(), 10)
        .unwrap();
    let mut paused = report();
    paused.lifecycle = "paused".into();
    paused.foreground = false;
    s.client_report(&h, "s", &o, "l", 1, 1, Some(paused), 20)
        .unwrap();
    assert!(s.client_live_target(h.id(), "s", "l", false, 21).is_err());
    let mut failed = report();
    failed.lifecycle = "failed".into();
    s.client_report(&h, "s", &o, "l", 1, 2, Some(failed), 30)
        .unwrap();
    assert!(s.client_live_target(h.id(), "s", "l", false, 31).is_ok());
    assert!(s.client_live_target(h.id(), "s", "l", true, 31).is_err());
    assert!(s
        .client_live_target(h.id(), "s", "l", false, 30 + LEASE_MS)
        .is_err());
    assert!(s
        .client_report(&h, "s", &o, "l", 1, 3, Some(report()), 30 + LEASE_MS)
        .is_err());
    s.client_connect(&h, "s", &o, "l", None, 2, report(), 30 + LEASE_MS)
        .unwrap();
    assert!(s
        .client_live_target(h.id(), "s", "l", true, 31 + LEASE_MS)
        .is_ok());
    s.client_report(&h, "s", &o, "l", 2, 1, None, 32 + LEASE_MS)
        .unwrap();
    assert!(s
        .client_live_target(h.id(), "s", "l", true, 33 + LEASE_MS)
        .is_err());
    assert!(s
        .client_connect(&h, "s", &o, "l", None, 2, report(), 33 + LEASE_MS)
        .is_err());
}
#[test]
fn restart_keeps_identity_but_requires_new_connection() {
    let path = std::env::temp_dir().join(format!("talia-clients-{}.db", std::process::id()));
    let mut s = Store::open(&path).unwrap();
    s.client_register(&"d".repeat(64), "Tablet", "android")
        .unwrap();
    let h = s.client_authenticate(&"d".repeat(64)).unwrap();
    s.client_connect(&h, "slot", &"e".repeat(64), "live", None, 1, report(), 10)
        .unwrap();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    s.clients_recover().unwrap();
    let h = s.client_authenticate(&"d".repeat(64)).unwrap();
    assert!(s
        .client_live_target(h.id(), "slot", "live", true, 11)
        .is_err());
    s.client_connect(&h, "slot", &"e".repeat(64), "live", None, 2, report(), 12)
        .unwrap();
    assert!(s
        .client_live_target(h.id(), "slot", "live", true, 13)
        .is_ok());
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn report_order_and_strict_schema() {
    let (mut s, h, o) = setup();
    s.client_connect(&h, "s", &o, "l", None, 1, report(), 0)
        .unwrap();
    let mut dirty = report();
    dirty.dirty = true;
    dirty.edit_revision = 1;
    s.client_report(&h, "s", &o, "l", 1, 2, Some(dirty), 1)
        .unwrap();
    assert!(s
        .client_report(&h, "s", &o, "l", 1, 1, Some(report()), 2)
        .is_err());
    assert!(s
        .client_report(&h, "s", &o, "l", 1, 3, Some(report()), 2)
        .is_err());
    assert!(serde_json::from_value::<Request>(json!({"op":"status","principal":"root"})).is_err());
}
