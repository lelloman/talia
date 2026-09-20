use super::*;
impl Store {
    fn audit_load(&self, id: u64) -> Result<AuditRecord> {
        let s: String = self
            .conn
            .query_row("SELECT body FROM agent_audit WHERE id=?", [id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or(ErrorCode::NotFound)?;
        Ok(serde_json::from_str(&s)?)
    }
    fn audit_insert(&self, mut record: AuditRecord) -> Result<AuditRecord> {
        self.conn.execute(
            "INSERT INTO agent_audit(principal,operation,status,body) VALUES(?,?,?,?)",
            params![
                record.principal,
                serde_json::to_string(&record.operation)?,
                serde_json::to_string(&record.status)?,
                "{}"
            ],
        )?;
        record.id = self.conn.last_insert_rowid() as u64;
        self.audit_update(&record)?;
        Ok(record)
    }
    fn audit_update(&self, record: &AuditRecord) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_audit SET status=?,body=? WHERE id=?",
            params![
                serde_json::to_string(&record.status)?,
                serde_json::to_string(record)?,
                record.id
            ],
        )?;
        Ok(())
    }
    fn audit_compare_update(&self, record: &AuditRecord, expected: Status) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE agent_audit SET status=?,body=? WHERE id=? AND status=?",
            params![
                serde_json::to_string(&record.status)?,
                serde_json::to_string(record)?,
                record.id,
                serde_json::to_string(&expected)?
            ],
        )?;
        if changed != 1 {
            return Err(ErrorCode::Conflict);
        }
        Ok(())
    }
    /// Admission commits before the caller may dispatch. Duplicate IDs never yield another permit.
    pub fn agent_admit(
        &mut self,
        session: &Session,
        request_id: &str,
        operation: Operation,
        targets: &[Target],
        arguments: &Value,
        before: &[Revision],
        now: i64,
    ) -> Result<Admission> {
        self.agent_grants(session)?;
        valid_id(request_id)?;
        revisions_valid(before)?;
        if targets.is_empty() || targets.len() > 256 || arguments.to_string().len() > 2_097_152 {
            return Err(ErrorCode::LimitExceeded);
        }
        if operation != Operation::DefinitionsSave
            && operation != Operation::DefinitionsValidate
            && targets.len() != 1
        {
            return Err(ErrorCode::InvalidInput);
        }
        for t in targets {
            valid_target(t)?;
            if !operation.accepts(t) {
                return Err(ErrorCode::InvalidInput);
            }
        }
        if before.iter().any(|r| !targets.contains(&r.target)) {
            return Err(ErrorCode::InvalidInput);
        }
        // HMAC prevents guessing low-entropy secret argument values from stored digests.
        let key: Vec<u8> = self.conn.query_row(
            "SELECT digest_key FROM agent_security WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let body = serde_json::to_vec(
            &json!({"principal":session.principal,"operation":operation,"targets":targets,"arguments":arguments}),
        )?;
        let signature = hex(hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, &key), &body).as_ref());
        self.conn.execute_batch("SAVEPOINT agent_admission")?;
        let result = (|| {
            let previous:Option<(String,u64)>=self.conn.query_row("SELECT signature,audit_id FROM agent_requests WHERE principal=? AND request_id=?",params![session.principal,request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((sig, id)) = previous {
                if sig == signature {
                    // Do not reveal an old result after its authority was revoked.
                    let old = self.audit_load(id)?;
                    if old.error != Some(ErrorCode::Forbidden) {
                        for t in &old.targets {
                            self.agent_require(session, old.operation, t)?;
                        }
                    }
                    return Ok(Admission {
                        record: old,
                        permit: None,
                    });
                }
                let rejected = AuditRecord {
                    id: 0,
                    principal: session.principal.clone(),
                    request_id: request_id.into(),
                    operation,
                    targets: targets.to_vec(),
                    digest: signature.clone(),
                    admitted_at: now,
                    updated_at: now,
                    status: Status::Failed,
                    error: Some(ErrorCode::Conflict),
                    // Conflicts are rejected before authorization; disclose no resolved state.
                    before: vec![],
                    after: vec![],
                };
                return Ok(Admission {
                    record: self.audit_insert(rejected)?,
                    permit: None,
                });
            }
            let mut denied = targets
                .iter()
                .find_map(|t| self.agent_require(session, operation, t).err());
            let active: u64 = self.conn.query_row(
                "SELECT count(*) FROM agent_audit WHERE status IN ('\"pending\"','\"running\"')",
                [],
                |r| r.get(0),
            )?;
            if denied.is_none() && active >= 128 {
                denied = Some(ErrorCode::LimitExceeded);
            }
            let record = AuditRecord {
                id: 0,
                principal: session.principal.clone(),
                request_id: request_id.into(),
                operation,
                targets: targets.to_vec(),
                digest: signature.clone(),
                admitted_at: now,
                updated_at: now,
                status: if denied.is_some() {
                    Status::Failed
                } else {
                    Status::Pending
                },
                error: denied,
                before: if denied.is_some() {
                    vec![]
                } else {
                    before.to_vec()
                },
                after: vec![],
            };
            let record = self.audit_insert(record)?;
            self.conn.execute(
                "INSERT INTO agent_requests VALUES(?,?,?,?)",
                params![session.principal, request_id, signature, record.id],
            )?;
            let permit = if denied.is_none() {
                Some(Permit {
                    audit_id: record.id,
                    signature: signature.clone(),
                    session: session.clone(),
                })
            } else {
                None
            };
            Ok(Admission { record, permit })
        })();
        if result.is_ok() {
            self.conn.execute_batch("RELEASE agent_admission")?;
        } else {
            self.conn
                .execute_batch("ROLLBACK TO agent_admission; RELEASE agent_admission")?;
        }
        result
    }
    /// Call immediately before the first dispatch, with no await between this and the effect.
    pub fn agent_start(&mut self, permit: &Permit, now: i64) -> Result<AuditRecord> {
        let mut record = self.audit_load(permit.audit_id)?;
        if record.principal != permit.session.principal
            || record.digest != permit.signature
            || record.status != Status::Pending
        {
            return Err(ErrorCode::Conflict);
        }
        for t in &record.targets {
            if let Err(e) = self.agent_require(&permit.session, record.operation, t) {
                record.status = Status::Failed;
                record.error = Some(e);
                record.updated_at = now;
                self.audit_compare_update(&record, Status::Pending)?;
                return Err(e);
            }
        }
        record.status = Status::Running;
        record.updated_at = now;
        self.audit_compare_update(&record, Status::Pending)?;
        Ok(record)
    }
    /// Recheck at every continuation/effect. Terminal or revoked invocations cannot dispatch.
    pub fn agent_check_permit(&self, permit: &Permit) -> Result<AuditRecord> {
        let record = self.audit_load(permit.audit_id)?;
        if record.principal != permit.session.principal
            || record.digest != permit.signature
            || record.status != Status::Running
        {
            return Err(ErrorCode::Cancelled);
        }
        for t in &record.targets {
            self.agent_require(&permit.session, record.operation, t)?;
        }
        Ok(record)
    }
    /// A known result may still be recorded after credential revocation; no new effect is authorized.
    pub fn agent_finish(
        &mut self,
        permit: &Permit,
        status: Status,
        error: Option<ErrorCode>,
        after: &[Revision],
        now: i64,
    ) -> Result<AuditRecord> {
        if !status.terminal()
            || status == Status::Complete && error.is_some()
            || status != Status::Complete && error.is_none()
        {
            return Err(ErrorCode::InvalidInput);
        }
        revisions_valid(after)?;
        let mut record = self.audit_load(permit.audit_id)?;
        if record.principal != permit.session.principal
            || record.digest != permit.signature
            || after.iter().any(|r| !record.targets.contains(&r.target))
        {
            return Err(ErrorCode::InvalidInput);
        }
        if record.status.terminal() {
            if record.status == status && record.error == error && record.after == after {
                return Ok(record);
            }
            return Err(ErrorCode::Conflict);
        }
        if record.status == Status::Pending && status == Status::Complete {
            return Err(ErrorCode::Conflict);
        }
        let previous = record.status;
        record.status = status;
        record.error = error;
        record.after = after.to_vec();
        record.updated_at = now;
        self.audit_compare_update(&record, previous)?;
        Ok(record)
    }
    /// Recovery happens once per server incarnation, before accepting agent traffic. Never replay.
    pub fn agent_recover(&mut self, now: i64) -> Result<usize> {
        let rows: Vec<String> = {
            let mut q = self.conn.prepare(
                "SELECT body FROM agent_audit WHERE status IN ('\"pending\"','\"running\"')",
            )?;
            let rows = q.query_map([], |r| r.get(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        let tx = self.conn.savepoint()?;
        for body in &rows {
            let mut r: AuditRecord = serde_json::from_str(body)?;
            r.status = if r.status == Status::Pending {
                Status::Failed
            } else {
                Status::Unknown
            };
            r.error = Some(if r.status == Status::Failed {
                ErrorCode::Cancelled
            } else {
                ErrorCode::Unknown
            });
            r.updated_at = now;
            tx.execute(
                "UPDATE agent_audit SET status=?,body=? WHERE id=?",
                params![
                    serde_json::to_string(&r.status)?,
                    serde_json::to_string(&r)?,
                    r.id
                ],
            )?;
        }
        tx.commit()?;
        Ok(rows.len())
    }
    pub fn agent_status(&self, session: &Session, request_id: &str) -> Result<AuditRecord> {
        self.agent_grants(session)?;
        valid_id(request_id)?;
        let id: Option<u64> = self
            .conn
            .query_row(
                "SELECT audit_id FROM agent_requests WHERE principal=? AND request_id=?",
                params![session.principal, request_id],
                |r| r.get(0),
            )
            .optional()?;
        let r = self.audit_load(id.ok_or(ErrorCode::NotFound)?)?;
        if r.error != Some(ErrorCode::Forbidden) {
            for t in &r.targets {
                self.agent_require(session, r.operation, t)?;
            }
        }
        Ok(r)
    }
    /// Scans bounded pages; cursor advances over hidden records without leaking their contents.
    pub fn agent_audit_list(
        &self,
        session: &Session,
        after: u64,
        limit: usize,
    ) -> Result<AuditPage> {
        if !(1..=100).contains(&limit) {
            return Err(ErrorCode::InvalidInput);
        }
        let grants = self.agent_grants(session)?;
        if !grants.iter().any(|g| g.actions.contains(&Action::Audit)) {
            return Err(ErrorCode::Forbidden);
        }
        let mut q = self
            .conn
            .prepare("SELECT id,body FROM agent_audit WHERE id>? ORDER BY id LIMIT 1000")?;
        let rows = q.query_map([after], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut records = vec![];
        let mut cursor = after;
        let mut count = 0;
        for row in rows {
            let (id, body) = row?;
            cursor = id;
            count += 1;
            let r: AuditRecord = serde_json::from_str(&body)?;
            if r.targets
                .iter()
                .all(|t| allows(&grants, r.operation.permission().0, Action::Audit, t))
            {
                records.push(r);
                if records.len() == limit {
                    break;
                }
            }
        }
        let more: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_audit WHERE id>?)",
            [cursor],
            |r| r.get(0),
        )?;
        Ok(AuditPage {
            records,
            next: if count > 0 && more {
                Some(cursor)
            } else {
                None
            },
        })
    }
    /// Host grants come from the loaded package, never from injected code or tool arguments.
    pub fn agent_live_effect(
        &self,
        permit: &Permit,
        operation: Operation,
        resource: &str,
        host: &HostGrants,
    ) -> Result<()> {
        let record = self.agent_check_permit(permit)?;
        if record.operation != Operation::LiveExecute {
            return Err(ErrorCode::Forbidden);
        }
        let host_ids = match operation {
            Operation::EngineRead | Operation::EngineSubscribe => &host.reads,
            Operation::EngineWrite | Operation::EngineSet => &host.writes,
            Operation::EngineRun => &host.runs,
            _ => return Err(ErrorCode::Forbidden),
        };
        if !host_ids.iter().any(|id| id == resource) {
            return Err(ErrorCode::Forbidden);
        }
        self.agent_require(
            &permit.session,
            operation,
            &Target::Resource {
                id: resource.into(),
            },
        )
    }
}
