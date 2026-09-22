//! Short-lived credentials issued only by the authenticated browser host.
use super::*;
fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn random() -> Result<String> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| ErrorCode::InternalError)?;
    Ok(hex(&bytes))
}
impl Store {
    pub fn user_key_issue(&mut self, subject: &str, auth_session: &str) -> Result<Value> {
        if auth_session.len() != 43 {
            return Err(ErrorCode::Unauthenticated);
        }
        let time = now();
        let tx = self.conn.savepoint()?;
        tx.execute("DELETE FROM user_agent_keys WHERE expires<=?", [time])?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM user_agent_keys WHERE subject=?",
            [subject],
            |r| r.get(0),
        )?;
        let total: i64 = tx.query_row("SELECT count(*) FROM user_agent_keys", [], |r| r.get(0))?;
        if count >= 10 || total >= 1024 {
            return Err(ErrorCode::LimitExceeded);
        }
        let token = random()?;
        let id = random()?;
        let expires = time + 3_600_000;
        tx.execute(
            "INSERT INTO user_agent_keys VALUES(?,?,?,?,?,?)",
            params![
                id,
                credential_digest(&token),
                subject,
                auth_session,
                time,
                expires
            ],
        )?;
        tx.commit()?;
        Ok(json!({"id":id,"key":token,"created":time,"expires":expires}))
    }
    pub fn user_keys(&self, subject: &str) -> Result<Value> {
        let mut q=self.conn.prepare("SELECT id,created,expires FROM user_agent_keys WHERE subject=? AND expires>? ORDER BY created DESC")?;
        let keys=q.query_map(params![subject,now()],|r|Ok(json!({"id":r.get::<_,String>(0)?,"created":r.get::<_,i64>(1)?,"expires":r.get::<_,i64>(2)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(json!({"keys":keys}))
    }
    pub fn user_key_revoke(&mut self, subject: &str, id: &str) -> Result<Value> {
        self.conn.execute(
            "DELETE FROM user_agent_keys WHERE id=? AND subject=?",
            params![id, subject],
        )?;
        Ok(json!({"revoked":true}))
    }
    pub fn user_key_info(&self, token: &str) -> Result<Value> {
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(ErrorCode::Unauthenticated);
        }
        self.user_key_digest_info(&credential_digest(token))
    }
    fn user_key_digest_info(&self, digest: &str) -> Result<Value> {
        self.conn.query_row("SELECT id,subject,auth_session,expires FROM user_agent_keys WHERE digest=? AND expires>?",params![digest,now()],|r|Ok(json!({"id":r.get::<_,String>(0)?,"subject":r.get::<_,String>(1)?,"authSession":r.get::<_,String>(2)?,"expires":r.get::<_,i64>(3)?}))).optional()?.ok_or(ErrorCode::Unauthenticated)
    }
    pub fn user_agent_authenticate(&self, token: &str) -> Result<Session> {
        let info = self.user_key_info(token)?;
        Ok(Session {
            principal: info["subject"].as_str().unwrap().into(),
            credential_digest: format!("user:{}", credential_digest(token)),
        })
    }
    pub(super) fn user_agent_grants(&self, session: &Session) -> Result<Vec<Grant>> {
        let digest = session
            .credential_digest
            .strip_prefix("user:")
            .ok_or(ErrorCode::Unauthenticated)?;
        let info = self.user_key_digest_info(digest)?;
        if info["subject"] != session.principal {
            return Err(ErrorCode::Unauthenticated);
        }
        // Viewers may only consume dashboards as presented, never query arbitrary engine data.
        if !self.user_admin(&session.principal)? {
            return Ok(vec![]);
        }
        Ok(
            serde_json::from_str::<Value>(include_str!("../../../deploy/operator-policy.json"))?
                ["grants"]
                .as_array()
                .ok_or(ErrorCode::InternalError)?
                .iter()
                .cloned()
                .map(serde_json::from_value)
                .collect::<std::result::Result<Vec<_>, _>>()?,
        )
    }
    pub fn user_agent_revalidate(&self, session: &Session) -> Result<()> {
        self.agent_grants(session).map(|_| ())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_expiring_revocable_and_current_role() {
        let mut s = Store::open(":memory:").unwrap();
        s.user_bootstrap("issuer#admin").unwrap();
        s.user_seen("issuer#viewer", "Viewer").unwrap();
        let k = s.user_key_issue("issuer#admin", &"a".repeat(43)).unwrap();
        let token = k["key"].as_str().unwrap();
        let session = s.user_agent_authenticate(token).unwrap();
        assert_eq!(session.principal(), "issuer#admin");
        assert!(s.agent_require_dashboard_admin(&session).is_ok());
        assert!(s.agent_authenticate(token).is_err());
        assert!(!s
            .user_keys("issuer#admin")
            .unwrap()
            .to_string()
            .contains(token));
        assert_eq!(s.user_keys("issuer#viewer").unwrap()["keys"], json!([]));
        s.user_key_revoke("issuer#viewer", k["id"].as_str().unwrap())
            .unwrap();
        assert!(s.user_key_info(token).is_ok());
        s.conn
            .execute(
                "UPDATE dashboard_users SET admin=0 WHERE subject='issuer#admin'",
                [],
            )
            .unwrap();
        assert!(s.agent_require_dashboard_admin(&session).is_err());
        s.user_key_revoke("issuer#admin", k["id"].as_str().unwrap())
            .unwrap();
        assert!(s.user_agent_revalidate(&session).is_err());
        let k = s.user_key_issue("issuer#viewer", &"b".repeat(43)).unwrap();
        let session = s
            .user_agent_authenticate(k["key"].as_str().unwrap())
            .unwrap();
        assert!(s.agent_require_dashboard_admin(&session).is_err());
        s.conn
            .execute("UPDATE user_agent_keys SET expires=0", [])
            .unwrap();
        assert!(s.user_agent_revalidate(&session).is_err());
    }
}
