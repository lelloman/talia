use super::*;
#[test]
fn saved_shared_packages_and_independent_assignments() {
    let mut s = Store::open(":memory:").unwrap();
    s.seed_dashboards().unwrap();
    let token = "a".repeat(64);
    s.client_register(&token, "Web", "web").unwrap();
    let h = s.client_authenticate(&token).unwrap();
    let owner = "b".repeat(64);
    let a = s.delivery_open(&h, "a", &owner).unwrap();
    s.delivery_open(&h, "b", &owner).unwrap();
    let old = s.delivery_prepare(&h, "a", &owner).unwrap();
    let mut assigned = a.assignment.clone();
    assigned.params = json!({"sidebar":true,"title":"Only A"});
    s.delivery_select(&h, "a", &owner, a.revision, &assigned)
        .unwrap();
    assert!(s.delivery_confirm(&h, "a", &owner, a.revision).is_err());
    assert!(s
        .delivery_select(&h, "a", &owner, a.revision, &assigned)
        .is_err());
    assert_eq!(
        s.delivery_prepare(&h, "a", &owner).unwrap().package["params"]["sidebar"],
        true
    );
    assert_eq!(
        s.delivery_prepare(&h, "b", &owner).unwrap().package["params"]["sidebar"],
        false
    );
    let default = s.assignment_get(h.id(), None).unwrap();
    assigned.dashboard_id = "monitoring".into();
    s.assignment_set(h.id(), None, default.revision, &assigned)
        .unwrap();
    assert_eq!(
        s.delivery_open(&h, "c", &owner)
            .unwrap()
            .assignment
            .dashboard_id,
        "monitoring"
    );
    assert_eq!(
        s.assignment_get(h.id(), Some("b"))
            .unwrap()
            .assignment
            .dashboard_id,
        "monitor"
    );
    s.catalog_save(&ChangeSet {
        expected_catalog_revision: s.catalog_revision().unwrap(),
        changes: vec![Change::Put {
            key: Key::new("ui", "notice"),
            document: json!({"source":"<Text id=\"Notice\" text=\"Updated\"/>"}),
            migration: None,
            initial: None,
        }],
    })
    .unwrap();
    let next = s.delivery_prepare(&h, "a", &owner).unwrap();
    assert_ne!(old.package["revision"], next.package["revision"]);
    assert_eq!(
        s.delivery_prepare(&h, "b", &owner).unwrap().package["revision"],
        next.package["revision"]
    );
    assert_eq!(old.package["params"]["sidebar"], false);
    assert_eq!(next.package["params"]["sidebar"], true);
    assert!(s.delivery_prepare(&h, "a", &"c".repeat(64)).is_err());
}
#[test]
fn deletion_does_not_resurrect_saved_package() {
    let mut s = Store::open(":memory:").unwrap();
    s.seed_dashboards().unwrap();
    let token = "d".repeat(64);
    s.client_register(&token, "Tablet", "android").unwrap();
    let h = s.client_authenticate(&token).unwrap();
    let o = "e".repeat(64);
    s.delivery_open(&h, "s", &o).unwrap();
    let pinned = s.delivery_prepare(&h, "s", &o).unwrap();
    s.catalog_save(&ChangeSet {
        expected_catalog_revision: s.catalog_revision().unwrap(),
        changes: vec![Change::Delete {
            key: Key::new("dashboard", "monitor"),
        }],
    })
    .unwrap();
    assert!(s.delivery_prepare(&h, "s", &o).is_err());
    assert!(s
        .delivery_confirm(&h, "s", &o, pinned.assignment.revision)
        .is_err());
    assert_eq!(pinned.package["id"], "monitor");
    s.seed_dashboards().unwrap();
    assert!(s.dashboard_package("monitor").is_err());
}
#[test]
fn assignments_survive_restart_and_seed_never_overwrites() {
    let path = std::env::temp_dir().join(format!("talia-delivery-{}.db", std::process::id()));
    let mut s = Store::open(&path).unwrap();
    s.seed_dashboards().unwrap();
    let credential = "f".repeat(64);
    s.client_register(&credential, "Saved", "web").unwrap();
    let h = s.client_authenticate(&credential).unwrap();
    let owner = "a".repeat(64);
    let a = s.delivery_open(&h, "slot", &owner).unwrap();
    let mut next = a.assignment;
    next.params = json!({"sidebar":true});
    let saved = s
        .delivery_select(&h, "slot", &owner, a.revision, &next)
        .unwrap();
    let prepared = s.delivery_prepare(&h, "slot", &owner).unwrap();
    drop(s);
    let mut s = Store::open(&path).unwrap();
    s.seed_dashboards().unwrap();
    let h = s.client_authenticate(&credential).unwrap();
    assert_eq!(s.delivery_open(&h, "slot", &owner).unwrap(), saved);
    assert_eq!(
        s.delivery_prepare(&h, "slot", &owner).unwrap().package,
        prepared.package
    );
    s.catalog_save(&ChangeSet {
        expected_catalog_revision: s.catalog_revision().unwrap(),
        changes: vec![
            Change::Delete {
                key: Key::new("dashboard", "monitor"),
            },
            Change::Delete {
                key: Key::new("dashboard", "monitoring"),
            },
        ],
    })
    .unwrap();
    s.seed_dashboards().unwrap();
    assert!(s.delivery_prepare(&h, "slot", &owner).is_err());
    drop(s);
    std::fs::remove_file(path).unwrap();
}
