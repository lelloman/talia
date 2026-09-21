use super::*;
impl Store {
    pub fn agent_catalog_list(
        &self,
        session: &Session,
        kind: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        if !(1..=100).contains(&limit) {
            return Err(ErrorCode::InvalidInput);
        }
        let grants = self.agent_grants(session)?;
        if !grants
            .iter()
            .any(|g| g.family == Family::Authoring && g.actions.contains(&Action::List))
        {
            return Err(ErrorCode::Forbidden);
        }
        let snapshot = self
            .catalog_snapshot()
            .map_err(|_| ErrorCode::StorageError)?;
        let context = json!(["definitions", kind, snapshot.catalog_revision]);
        let offset = cursor
            .map(|c| self.agent_cursor_read(session, &context, c))
            .transpose()?
            .unwrap_or(0) as usize;
        let records: Vec<_> = snapshot
            .records
            .into_iter()
            .filter(|r| {
                kind.is_none_or(|k| r.key.kind == k)
                    && allows(
                        &grants,
                        Family::Authoring,
                        Action::List,
                        &Target::Definition { key: r.key.clone() },
                    )
            })
            .map(|r| json!({"key":r.key,"revision":r.revision}))
            .collect();
        if offset > records.len() {
            return Err(ErrorCode::InvalidInput);
        }
        let end = (offset + limit).min(records.len());
        let next = if end < records.len() {
            Some(self.agent_cursor(session, &context, end as u64)?)
        } else {
            None
        };
        Ok(
            json!({"catalogRevision":snapshot.catalog_revision,"records":records[offset..end],"nextCursor":next}),
        )
    }
    pub fn agent_catalog_read(
        &self,
        session: &Session,
        keys: &[catalog::Key],
    ) -> Result<catalog::Snapshot> {
        if keys.is_empty()
            || keys.len() > 32
            || keys.iter().collect::<BTreeSet<_>>().len() != keys.len()
        {
            return Err(ErrorCode::InvalidInput);
        }
        for key in keys {
            self.agent_require(
                session,
                Operation::DefinitionsRead,
                &Target::Definition { key: key.clone() },
            )?;
        }
        let mut snapshot = self
            .catalog_snapshot()
            .map_err(|_| ErrorCode::StorageError)?;
        snapshot.records.retain(|r| keys.contains(&r.key));
        if snapshot.records.len() != keys.len() {
            return Err(ErrorCode::NotFound);
        }
        if serde_json::to_vec(&snapshot)?.len() > 2_097_152 {
            return Err(ErrorCode::LimitExceeded);
        }
        Ok(snapshot)
    }
    fn cursor_key(&self) -> Result<hmac::Key> {
        let bytes: Vec<u8> = self.conn.query_row(
            "SELECT digest_key FROM agent_security WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        Ok(hmac::Key::new(hmac::HMAC_SHA256, &bytes))
    }
    fn cursor_body(&self, session: &Session, context: &Value, offset: u64) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&json!([
            "mcp-cursor-v1",
            session.principal,
            session.credential_digest,
            self.agent_grants(session)?,
            context,
            offset
        ]))?)
    }
    pub(crate) fn agent_cursor(
        &self,
        session: &Session,
        context: &Value,
        offset: u64,
    ) -> Result<String> {
        let signature = hmac::sign(
            &self.cursor_key()?,
            &self.cursor_body(session, context, offset)?,
        );
        Ok(format!("{offset}.{}", hex(signature.as_ref())))
    }
    pub(crate) fn agent_cursor_read(
        &self,
        session: &Session,
        context: &Value,
        cursor: &str,
    ) -> Result<u64> {
        if cursor.len() > 85 {
            return Err(ErrorCode::InvalidInput);
        }
        let (offset, signature) = cursor.split_once('.').ok_or(ErrorCode::InvalidInput)?;
        let offset: u64 = offset.parse().map_err(|_| ErrorCode::InvalidInput)?;
        if signature.len() != 64 || !signature.is_ascii() {
            return Err(ErrorCode::InvalidInput);
        }
        let signature: Vec<u8> = (0..64)
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&signature[i..i + 2], 16).map_err(|_| ErrorCode::InvalidInput)
            })
            .collect::<Result<_>>()?;
        hmac::verify(
            &self.cursor_key()?,
            &self.cursor_body(session, context, offset)?,
            &signature,
        )
        .map_err(|_| ErrorCode::Conflict)?;
        Ok(offset)
    }
    /// Offline operator setup is atomic; no guest/MCP route exposes this method.
    pub fn agent_provision(
        &mut self,
        id: &str,
        expected: u64,
        enabled: bool,
        grants: &[Grant],
        ceiling: Option<&[Grant]>,
        issue: bool,
    ) -> Result<Option<String>> {
        self.conn.execute_batch("SAVEPOINT agent_provision")?;
        let result = (|| {
            self.agent_policy_set(id, expected, enabled, grants)?;
            if let Some(ceiling) = ceiling {
                self.agent_ceiling_set(ceiling)?;
            }
            if issue {
                self.agent_credential_issue(id).map(Some)
            } else {
                Ok(None)
            }
        })();
        if result.is_ok() {
            self.conn.execute_batch("RELEASE agent_provision")?;
        } else {
            self.conn
                .execute_batch("ROLLBACK TO agent_provision; RELEASE agent_provision")?;
        }
        result
    }
}
