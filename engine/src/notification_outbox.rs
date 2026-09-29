//! Store push outbox. SQLite triggers capture report/incident changes in the source transaction.
use crate::store::{Result, Store};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub owner: String,
    pub subject: String,
    pub session: String,
    pub application: String,
}
impl Store {
    pub fn notification_subscriptions(&self) -> Result<Vec<Subscription>> {
        let mut q = self
            .conn
            .prepare("SELECT id,owner,subject,session,application FROM store_push_subscriptions")
            .map_err(|e| e.to_string())?;
        let rows = q
            .query_map([], |r| {
                Ok(Subscription {
                    id: r.get(0)?,
                    owner: r.get(1)?,
                    subject: r.get(2)?,
                    session: r.get(3)?,
                    application: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }
    pub fn notification_subscribe(&self, s: &Subscription) -> Result<()> {
        self.conn.execute("INSERT INTO store_push_subscriptions VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET session=excluded.session",params![s.id,s.owner,s.subject,s.session,s.application]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn notification_forget(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM store_push_subscriptions WHERE id=?", [id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn notification_pending(&self) -> Result<Vec<(String, Value)>> {
        let mut q=self.conn.prepare("SELECT id,body FROM store_push_outbox WHERE status='pending' ORDER BY at,id LIMIT 32").map_err(|e|e.to_string())?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        rows.map(|r| {
            let (id, body) = r.map_err(|e| e.to_string())?;
            Ok((id, serde_json::from_str(&body).map_err(|e| e.to_string())?))
        })
        .collect()
    }
    pub fn notification_result(&self, id: &str, status: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE store_push_outbox SET status=?,body='{}' WHERE id=?",
                params![status, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn notification_prune(&self) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM store_push_outbox WHERE status<>'pending' AND at<unixepoch()-2592000",
                [],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn source_transaction_and_outbox_commit_together() {
        let store = Store::open(":memory:").unwrap();
        store
            .notification_subscribe(&Subscription {
                id: "sub".into(),
                owner: "issuer#alice".into(),
                subject: "alice".into(),
                session: "session".into(),
                application: "com.lelloman.talia".into(),
            })
            .unwrap();
        let alert = json!({"key":"host","occurrence":1,"revision":1,"active":true,"severity":"critical","stage":"firing","message":"Host unavailable"});
        store.conn.execute_batch("SAVEPOINT test_change").unwrap();
        store
            .conn
            .execute(
                "INSERT INTO alert_entities VALUES('alert','host',?)",
                [alert.to_string()],
            )
            .unwrap();
        assert_eq!(store.notification_pending().unwrap().len(), 1);
        store
            .conn
            .execute_batch("ROLLBACK TO test_change;RELEASE test_change")
            .unwrap();
        assert!(store.notification_pending().unwrap().is_empty());
        store
            .conn
            .execute(
                "INSERT INTO alert_entities VALUES('alert','host',?)",
                [alert.to_string()],
            )
            .unwrap();
        let mut recovery = alert.clone();
        recovery["active"] = json!(false);
        recovery["revision"] = json!(2);
        store
            .conn
            .execute(
                "UPDATE alert_entities SET body=? WHERE id='host'",
                [recovery.to_string()],
            )
            .unwrap();
        let events = store.notification_pending().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].1["revision"], 2);
        assert_eq!(events[1].1["payload"]["active"], 0);
        // Repeating a source write cannot duplicate a notification.
        store
            .conn
            .execute(
                "UPDATE alert_entities SET body=? WHERE id='host'",
                [recovery.to_string()],
            )
            .unwrap();
        assert_eq!(store.notification_pending().unwrap().len(), 2);
        store.notification_forget("sub").unwrap();
        assert!(store.notification_pending().unwrap().is_empty());
    }
    #[test]
    fn report_previews_do_not_publish_and_severity_is_not_execution_status() {
        let store = Store::open(":memory:").unwrap();
        store
            .notification_subscribe(&Subscription {
                id: "sub".into(),
                owner: "issuer#alice".into(),
                subject: "alice".into(),
                session: "session".into(),
                application: "com.lelloman.talia".into(),
            })
            .unwrap();
        let mut report = json!({"send":false,"definition":{"id":"daily"},"content":{"severity":"warning","subject":"Daily report","summary":"One host needs attention"}});
        store
            .conn
            .execute(
                "INSERT INTO report_runs VALUES('preview','daily','completed',0,?)",
                [report.to_string()],
            )
            .unwrap();
        assert!(store.notification_pending().unwrap().is_empty());
        report["send"] = json!(true);
        store
            .conn
            .execute(
                "INSERT INTO report_runs VALUES('real','daily','running',0,?)",
                [report.to_string()],
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE report_runs SET status='completed' WHERE id='real'",
                [],
            )
            .unwrap();
        let events = store.notification_pending().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["level"], "warning");
        assert_eq!(events[0].1["payload"]["status"], "completed");
        store
            .conn
            .execute(
                "UPDATE report_runs SET status='completed' WHERE id='real'",
                [],
            )
            .unwrap();
        assert_eq!(store.notification_pending().unwrap().len(), 1);
    }
}
