use crate::value;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, time::Duration};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub id: String,
    pub definition: String,
    pub params: Value,
    pub state: Value,
    pub value: Value,
    pub has_value: bool,
    pub timestamp: i64,
    pub quality: String,
    pub revision: u64,
    pub generation: u64,
    pub history_count: u32,
    pub history_age_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub id: String,
    pub version: u64,
    pub source: String,
    pub kind: String,
    pub value_schema: String,
    pub state_schema: String,
    pub dependencies: Vec<String>,
    #[serde(default = "shared_reads")]
    pub read_policy: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub value: Value,
    pub timestamp: i64,
    pub quality: String,
    pub revision: u64,
}
fn shared_reads() -> String {
    "shared".into()
}
pub struct Store {
    pub(crate) conn: Connection,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut conn = Connection::open(path).map_err(err)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
        )
        .map_err(err)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        if version > 4 {
            return Err("database schema newer than engine".into());
        }
        if version == 0 {
            let tx = conn.transaction().map_err(err)?;
            tx.execute_batch("CREATE TABLE definitions(id TEXT PRIMARY KEY, body TEXT NOT NULL);
CREATE TABLE instances(id TEXT PRIMARY KEY, definition TEXT NOT NULL REFERENCES definitions(id), body TEXT NOT NULL);
CREATE TABLE history(seq INTEGER PRIMARY KEY AUTOINCREMENT, instance TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE, timestamp INTEGER NOT NULL, body TEXT NOT NULL);
CREATE INDEX history_instance ON history(instance,seq);
CREATE TABLE actions(id TEXT PRIMARY KEY, request TEXT NOT NULL, status TEXT NOT NULL, outcome TEXT);
CREATE TABLE metadata(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
INSERT INTO metadata VALUES('revision',0); PRAGMA user_version=1;").map_err(err)?;
            tx.commit().map_err(err)?;
        }
        if version < 2 {
            let tx = conn.transaction().map_err(err)?;
            tx.execute_batch("CREATE TABLE monitoring_config(id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);
CREATE TABLE monitoring_state(id TEXT PRIMARY KEY, body TEXT NOT NULL);
PRAGMA user_version=2;").map_err(err)?;
            tx.commit().map_err(err)?;
        }
        if version < 3 {
            let tx = conn.transaction().map_err(err)?;
            tx.execute_batch("CREATE TABLE monitoring_runs(id TEXT PRIMARY KEY, instance TEXT NOT NULL, status TEXT NOT NULL, body TEXT NOT NULL);
CREATE INDEX monitoring_runs_instance ON monitoring_runs(instance,status);
CREATE TABLE monitoring_requests(id TEXT PRIMARY KEY, signature TEXT NOT NULL, body TEXT NOT NULL);
CREATE TABLE monitoring_events(seq INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL);
INSERT INTO metadata VALUES('run_sequence',0);
PRAGMA user_version=3;").map_err(err)?;
            tx.commit().map_err(err)?;
        }
        if version < 4 {
            conn.execute_batch("BEGIN IMMEDIATE; ALTER TABLE monitoring_events ADD COLUMN depth INTEGER NOT NULL DEFAULT 0; PRAGMA user_version=4; COMMIT;").map_err(err)?;
        }
        Ok(Self { conn })
    }
    pub fn definition(&self, id: &str) -> Result<Definition> {
        let s: String = self
            .conn
            .query_row("SELECT body FROM definitions WHERE id=?", [id], |r| {
                r.get(0)
            })
            .map_err(err)?;
        serde_json::from_str(&s).map_err(err)
    }
    pub fn definitions(&self) -> Result<Vec<Definition>> {
        let mut q = self
            .conn
            .prepare("SELECT body FROM definitions ORDER BY id")
            .map_err(err)?;
        let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
        rows.map(|s| serde_json::from_str(&s.map_err(err)?).map_err(err))
            .collect()
    }
    pub fn instance(&self, id: &str) -> Result<Instance> {
        let s: String = self
            .conn
            .query_row("SELECT body FROM instances WHERE id=?", [id], |r| r.get(0))
            .map_err(err)?;
        serde_json::from_str(&s).map_err(err)
    }
    pub fn instances(&self) -> Result<Vec<Instance>> {
        let mut q = self
            .conn
            .prepare("SELECT body FROM instances ORDER BY id")
            .map_err(err)?;
        let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
        rows.map(|s| serde_json::from_str(&s.map_err(err)?).map_err(err))
            .collect()
    }
    pub fn revision(&self) -> Result<u64> {
        self.conn
            .query_row("SELECT value FROM metadata WHERE key='revision'", [], |r| {
                r.get(0)
            })
            .map_err(err)
    }
    pub fn create_definition(&mut self, d: &Definition) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO definitions VALUES(?,?)",
                params![d.id, serde_json::to_string(d).map_err(err)?],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn create_instance(&mut self, i: &Instance) -> Result<()> {
        self.check_instance(i)?;
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO instances VALUES(?,?,?)",
            params![i.id, i.definition, serde_json::to_string(i).map_err(err)?],
        )
        .map_err(err)?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(())
    }
    pub fn check_instance(&self, i: &Instance) -> Result<()> {
        let d = self.definition(&i.definition)?;
        value::validate(&i.params)?;
        if !value::matches_schema(&i.state, &d.state_schema)
            || i.has_value && !value::matches_schema(&i.value, &d.value_schema)
        {
            return Err("instance schema mismatch".into());
        }
        if i.history_count > 100000 || i.history_age_ms < 0 {
            return Err("history bounds".into());
        }
        Ok(())
    }
    /// Only short synchronous mutations run under this transaction. Caller has already awaited I/O.
    pub fn commit(
        &mut self,
        expected: u64,
        next: &Instance,
        sample: bool,
        now: i64,
    ) -> Result<u64> {
        self.check_instance(next)?;
        let old = self.instance(&next.id)?;
        if old.revision != expected
            || old.generation != next.generation
            || old.definition != next.definition
        {
            return Err("conflict".into());
        }
        if next.revision != expected + 1 {
            return Err("revision must increment".into());
        }
        let track = self
            .monitoring_config()?
            .definitions
            .iter()
            .any(|d| d.kind == "watch");
        if track
            && self
                .conn
                .query_row("SELECT count(*) FROM monitoring_events", [], |r| {
                    r.get::<_, u64>(0)
                })
                .map_err(err)?
                >= 10000
        {
            return Err("Watch observation backlog full".into());
        }
        let tx = self.conn.savepoint().map_err(err)?;
        tx.execute(
            "UPDATE instances SET body=? WHERE id=?",
            params![serde_json::to_string(next).map_err(err)?, next.id],
        )
        .map_err(err)?;
        if track {
            tx.execute(
                "INSERT INTO monitoring_events(body) VALUES(?)",
                [serde_json::to_string(next).map_err(err)?],
            )
            .map_err(err)?;
        }
        if sample && next.has_value && next.history_count > 0 && next.history_age_ms > 0 {
            tx.execute(
                "INSERT INTO history(instance,timestamp,body) VALUES(?,?,?)",
                params![
                    next.id,
                    next.timestamp,
                    serde_json::to_string(&Sample {
                        value: next.value.clone(),
                        timestamp: next.timestamp,
                        quality: next.quality.clone(),
                        revision: next.revision
                    })
                    .map_err(err)?
                ],
            )
            .map_err(err)?;
        }
        tx.execute("DELETE FROM history WHERE instance=? AND (timestamp<? OR seq NOT IN (SELECT seq FROM history WHERE instance=? ORDER BY seq DESC LIMIT ?))",params![next.id,now.saturating_sub(next.history_age_ms),next.id,next.history_count]).map_err(err)?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])
            .map_err(err)?;
        let revision = tx
            .query_row("SELECT value FROM metadata WHERE key='revision'", [], |r| {
                r.get(0)
            })
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(revision)
    }
    pub fn history(&self, id: &str, now: i64) -> Result<Vec<Sample>> {
        let i = self.instance(id)?;
        let mut q=self.conn.prepare("SELECT body FROM history WHERE instance=? AND timestamp>=? ORDER BY seq DESC LIMIT ?").map_err(err)?;
        let rows = q
            .query_map(
                params![id, now.saturating_sub(i.history_age_ms), i.history_count],
                |r| r.get::<_, String>(0),
            )
            .map_err(err)?;
        rows.map(|s| serde_json::from_str(&s.map_err(err)?).map_err(err))
            .collect()
    }
    pub fn backup(&self, path: &Path) -> Result<()> {
        if path.exists() {
            return Err("backup destination exists".into());
        }
        let mut dest = Connection::open(path).map_err(err)?;
        rusqlite::backup::Backup::new(&self.conn, &mut dest)
            .map_err(err)?
            .run_to_completion(16, Duration::from_millis(5), None)
            .map_err(err)?;
        Ok(())
    }
    pub fn action(&self, id: &str) -> Result<Option<(String, String, Option<String>)>> {
        use rusqlite::OptionalExtension;
        self.conn
            .query_row(
                "SELECT request,status,outcome FROM actions WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(err)
    }
    pub fn accept_action(&self, id: &str, request: &str) -> Result<bool> {
        if let Some((r, _, _)) = self.action(id)? {
            if r != request {
                return Err("action argument conflict".into());
            }
            return Ok(false);
        }
        self.conn
            .execute(
                "INSERT INTO actions VALUES(?,?,'accepted',NULL)",
                params![id, request],
            )
            .map_err(err)?;
        Ok(true)
    }
    pub fn finish_action(&self, id: &str, status: &str, outcome: &str) -> Result<()> {
        if !["complete", "failed", "unknown"].contains(&status) {
            return Err("action status".into());
        }
        self.conn
            .execute(
                "UPDATE actions SET status=?,outcome=? WHERE id=?",
                params![status, outcome, id],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn recover_actions(&self) -> Result<()> {
        self.conn
            .execute(
                "UPDATE actions SET status='unknown' WHERE status='accepted'",
                [],
            )
            .map_err(err)?;
        Ok(())
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn seed(s: &mut Store) -> Instance {
        let d = Definition {
            id: "stored".into(),
            version: 1,
            source: "".into(),
            kind: "stored".into(),
            value_schema: "number".into(),
            state_schema: "any".into(),
            dependencies: vec![],
            read_policy: "shared".into(),
        };
        s.create_definition(&d).unwrap();
        let i = Instance {
            id: "metric".into(),
            definition: d.id,
            params: value::undefined(),
            state: value::undefined(),
            value: value::number(1.0),
            has_value: true,
            timestamp: 100,
            quality: "good".into(),
            revision: 1,
            generation: 1,
            history_count: 2,
            history_age_ms: 100,
        };
        s.create_instance(&i).unwrap();
        i
    }
    #[test]
    fn commits_history_and_conflicts() {
        let mut s = Store::open(":memory:").unwrap();
        let mut i = seed(&mut s);
        for n in 2..6 {
            i.revision = n;
            i.value = value::number(n as f64);
            i.timestamp = 100 + n as i64;
            s.commit(n - 1, &i, true, 110).unwrap();
        }
        assert_eq!(s.history("metric", 110).unwrap().len(), 2);
        assert!(s.history("metric", 1000).unwrap().is_empty());
        assert!(s.commit(1, &i, true, 110).is_err());
        assert_eq!(s.instance("metric").unwrap(), i);
    }
    #[test]
    fn action_recovery() {
        let s = Store::open(":memory:").unwrap();
        assert!(s.accept_action("a", "x").unwrap());
        assert!(!s.accept_action("a", "x").unwrap());
        assert!(s.accept_action("a", "y").is_err());
        s.recover_actions().unwrap();
        assert_eq!(s.action("a").unwrap().unwrap().1, "unknown");
    }
}

#[cfg(test)]
mod crash_tests {
    use super::*;
    #[test]
    fn crash_child() {
        let Ok(path) = std::env::var("TALIA_CRASH_DB") else {
            return;
        };
        let mode = std::env::var("TALIA_CRASH_MODE").unwrap();
        let mut s = Store::open(&path).unwrap();
        let mut i = super::tests::seed(&mut s);
        s.accept_action("uncertain", "effect").unwrap();
        if mode == "before" {
            s.conn
                .execute_batch(
                    "BEGIN IMMEDIATE; UPDATE metadata SET value=99 WHERE key='revision';",
                )
                .unwrap();
        } else {
            i.revision += 1;
            i.value = value::number(f64::INFINITY);
            s.commit(1, &i, true, 100).unwrap();
        }
        std::fs::write(format!("{path}.ready"), "ready").unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }
    #[test]
    fn process_crash_and_backup() {
        for mode in ["before", "after"] {
            let path =
                std::env::temp_dir().join(format!("talia-store-{}-{mode}.db", std::process::id()));
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "store::crash_tests::crash_child"])
                .env("TALIA_CRASH_DB", &path)
                .env("TALIA_CRASH_MODE", mode)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let ready = format!("{}.ready", path.display());
            let until = std::time::Instant::now() + Duration::from_secs(10);
            while !Path::new(&ready).exists() {
                assert!(std::time::Instant::now() < until);
                std::thread::sleep(Duration::from_millis(10));
            }
            child.kill().unwrap();
            child.wait().unwrap();
            let s = Store::open(&path).unwrap();
            assert_eq!(s.revision().unwrap(), if mode == "before" { 1 } else { 2 });
            assert_eq!(s.instance("metric").unwrap().timestamp, 100);
            assert_eq!(
                s.instance("metric").unwrap().value,
                value::number(if mode == "before" { 1.0 } else { f64::INFINITY })
            );
            s.recover_actions().unwrap();
            assert_eq!(s.action("uncertain").unwrap().unwrap().1, "unknown");
            let backup = path.with_extension("backup");
            s.backup(&backup).unwrap();
            let restored = Store::open(&backup).unwrap();
            assert_eq!(restored.instances().unwrap(), s.instances().unwrap());
            assert_eq!(
                restored.action("uncertain").unwrap(),
                s.action("uncertain").unwrap()
            );
            assert!(s.backup(&backup).is_err());
            drop(restored);
            drop(s);
            for p in [path, backup, ready.into()] {
                std::fs::remove_file(p).unwrap();
            }
        }
    }
}
