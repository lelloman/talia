//! First-install welcome dashboard. Refuses to overwrite an existing catalog.
use serde_json::json;
use talia_engine::{catalog::{Change,ChangeSet,Key},store::Store};
fn main()->Result<(),String>{
 let path=std::env::args().nth(1).ok_or("usage: talia-bootstrap DATABASE")?;
 let lock=std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(format!("{path}.lock")).map_err(|e|e.to_string())?;
 lock.try_lock().map_err(|_|"stop the engine before bootstrap")?;
 let mut store=Store::open(path)?;
 if store.catalog_revision()?!=0{return Err("catalog already initialized; use MCP to edit it".into());}
 store.catalog_save(&ChangeSet{expected_catalog_revision:0,changes:vec![Change::Put{key:Key::new("dashboard","monitor"),document:json!({"ui":"<Dashboard id=\"monitor\"><Screen id=\"welcome\"><Column id=\"content\" padding=\"16dp\" gap=\"12dp\"><Text id=\"title\" text=\"Talìa is online\" /><Text id=\"description\" text=\"First deployment. Monitoring sources and dashboards can now be configured through MCP.\" /><Text id=\"coverage\" text=\"Homelab monitoring has not been migrated.\" /></Column></Screen><Surface id=\"surface\"><ScreenRef id=\"home\" screen=\"welcome\" /></Surface></Dashboard>","view_model":"defineVM({initial:()=>({}),actions:{}});","grants":{"reads":[],"writes":[],"runs":[]}}),migration:None,initial:None}]}).map_err(|e|e.message)?;
 println!("welcome dashboard initialized");Ok(())
}
