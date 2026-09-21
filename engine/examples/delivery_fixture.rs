//! Offline test fixture editor. Refuses to mutate a database owned by a running server.
use serde_json::json;
use talia_engine::{
    catalog::{Change, ChangeSet, Key},
    store::Store,
};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let action = &args[2];
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(format!("{path}.lock"))
        .unwrap();
    lock.try_lock().expect("stop fixture server before editing");
    let mut s = Store::open(path).unwrap();
    s.seed_dashboards().unwrap();
    let key = Key::new("dashboard", "monitor");
    let source = s
        .catalog_snapshot()
        .unwrap()
        .records
        .into_iter()
        .find(|r| r.key == key)
        .map(|r| r.document);
    let changes = match action.as_str() {
        "second" => vec![Change::Put {
            key: Key::new("dashboard", "secondary"),
            document: source.unwrap(),
            migration: None,
            initial: None,
        }],
        "update" => vec![Change::Put {
            key: Key::new("ui", "notice"),
            document: json!({"source":"<Text id=\"Notice\" text=\"Saved shared update\"/>"}),
            migration: None,
            initial: None,
        }],
        "delete" => vec![Change::Delete { key }],
        _ => panic!("unknown fixture action"),
    };
    s.catalog_save(&ChangeSet {
        expected_catalog_revision: s.catalog_revision().unwrap(),
        changes,
    })
    .unwrap();
}
