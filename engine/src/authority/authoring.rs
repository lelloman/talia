use super::*;
#[derive(Debug, Serialize)]
pub struct Saved {
    pub audit: AuditRecord,
    pub receipt: Option<catalog::Receipt>,
}
fn error_code(d: &catalog::Diagnostic) -> ErrorCode {
    match d.code.as_str() {
        "conflict" => ErrorCode::Conflict,
        "not_found" => ErrorCode::NotFound,
        "invalid_input" => ErrorCode::InvalidInput,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "storage_error" => ErrorCode::StorageError,
        _ => ErrorCode::ValidationFailed,
    }
}
fn targets(set: &catalog::ChangeSet) -> Vec<Target> {
    set.changes
        .iter()
        .map(|change| Target::Definition {
            key: match change {
                catalog::Change::Put { key, .. } | catalog::Change::Delete { key } => key.clone(),
            },
        })
        .collect()
}
impl Store {
    pub fn agent_catalog_snapshot(&self, session: &Session) -> Result<catalog::Snapshot> {
        let grants = self.agent_grants(session)?;
        if !grants
            .iter()
            .any(|g| g.family == Family::Authoring && g.actions.contains(&Action::Read))
        {
            return Err(ErrorCode::Forbidden);
        }
        let mut snapshot = self.catalog_snapshot().map_err(|d| error_code(&d))?;
        snapshot.records.retain(|r| {
            allows(
                &grants,
                Family::Authoring,
                Action::Read,
                &Target::Definition { key: r.key.clone() },
            )
        });
        Ok(snapshot)
    }
    pub fn agent_catalog_validate(
        &mut self,
        session: &Session,
        set: &catalog::ChangeSet,
    ) -> Result<catalog::Validation> {
        if set.changes.is_empty() {
            return Err(ErrorCode::InvalidInput);
        }
        for target in targets(set) {
            self.agent_require(session, Operation::DefinitionsValidate, &target)?;
        }
        self.authoring_references(session, set)?;
        let mut result = self.catalog_validate(set);
        // Exception messages can contain data from engine migration state. Do not forward them.
        for diagnostic in &mut result.diagnostics {
            diagnostic.message = match error_code(diagnostic) {
                ErrorCode::Conflict => "catalog revision conflict",
                ErrorCode::StorageError => "storage unavailable",
                ErrorCode::LimitExceeded => "authoring limit exceeded",
                _ => "definition validation failed",
            }
            .into();
            if diagnostic.key.as_ref().is_some_and(|key| {
                self.agent_require(
                    session,
                    Operation::DefinitionsRead,
                    &Target::Definition { key: key.clone() },
                )
                .is_err()
            }) {
                diagnostic.key = None;
                diagnostic.path = None;
                diagnostic.line = None;
                diagnostic.column = None;
            }
        }
        if result.valid {
            // Ceiling validation requires linked packages; stage and roll back exactly as a save.
            self.conn
                .execute_batch("SAVEPOINT agent_validate_ceiling")?;
            let check = (|| {
                let receipt = self.catalog_save(set).map_err(|d| error_code(&d))?;
                self.authoring_ceiling(&receipt)
            })();
            self.conn.execute_batch(
                "ROLLBACK TO agent_validate_ceiling; RELEASE agent_validate_ceiling",
            )?;
            check?;
        }
        Ok(result)
    }
    /// Admission is durable first; catalog activation and the success record share one transaction.
    pub fn agent_catalog_save(
        &mut self,
        session: &Session,
        request_id: &str,
        set: &catalog::ChangeSet,
        now: i64,
    ) -> Result<Saved> {
        let targets = targets(set);
        let revision = self
            .catalog_revision()
            .map_err(|_| ErrorCode::StorageError)?;
        let before: Vec<_> = targets
            .iter()
            .map(|t| Revision {
                target: t.clone(),
                revision: revision.to_string(),
            })
            .collect();
        let admission = self.agent_admit(
            session,
            request_id,
            Operation::DefinitionsSave,
            &targets,
            &serde_json::to_value(set)?,
            &before,
            now,
        )?;
        let Some(permit) = admission.permit else {
            return Ok(Saved {
                audit: admission.record,
                receipt: None,
            });
        };
        self.agent_start(&permit, now)?;
        self.conn.execute_batch("SAVEPOINT agent_catalog_effect")?;
        let result = (|| {
            self.agent_check_permit(&permit)?;
            self.authoring_references(session, set)?;
            let mut receipt = self.catalog_save(set).map_err(|d| error_code(&d))?;
            self.authoring_ceiling(&receipt)?;
            let after: Vec<_> = targets
                .iter()
                .map(|t| Revision {
                    target: t.clone(),
                    revision: receipt.catalog_revision.to_string(),
                })
                .collect();
            let audit = self.agent_finish(&permit, Status::Complete, None, &after, now)?;
            // Shared changes may affect packages outside this agent's discovery scope.
            receipt.packages.retain(|id, _| {
                self.agent_require(
                    session,
                    Operation::DefinitionsRead,
                    &Target::Definition {
                        key: catalog::Key::new("dashboard", id),
                    },
                )
                .is_ok()
            });
            Ok(Saved {
                audit,
                receipt: Some(receipt),
            })
        })();
        match result {
            Ok(value) => {
                self.conn.execute_batch("RELEASE agent_catalog_effect")?;
                Ok(value)
            }
            Err(code) => {
                self.conn.execute_batch(
                    "ROLLBACK TO agent_catalog_effect; RELEASE agent_catalog_effect",
                )?;
                let audit = self.agent_finish(&permit, Status::Failed, Some(code), &[], now)?;
                Ok(Saved {
                    audit,
                    receipt: None,
                })
            }
        }
    }
    fn authoring_references(&self, session: &Session, set: &catalog::ChangeSet) -> Result<()> {
        for change in &set.changes {
            if let catalog::Change::Put { document, .. } = change {
                if let Some(references) = document.get("references").and_then(Value::as_array) {
                    for reference in references {
                        let key: catalog::Key = serde_json::from_value(reference.clone())?;
                        self.agent_require(
                            session,
                            Operation::DefinitionsRead,
                            &Target::Definition { key },
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
    fn authoring_ceiling(&self, receipt: &catalog::Receipt) -> Result<()> {
        let ceiling = self.agent_ceiling()?;
        for id in receipt.packages.keys() {
            let package = self
                .dashboard_package(id)
                .map_err(|_| ErrorCode::StorageError)?;
            let grants: HostGrants = serde_json::from_value(
                package
                    .get("grants")
                    .cloned()
                    .unwrap_or_else(|| json!({"reads":["value"],"writes":["value"]})),
            )?;
            for (action, resources) in [
                (Action::Read, grants.reads),
                (Action::Write, grants.writes),
                (Action::Run, grants.runs),
            ] {
                for id in resources {
                    if !allows(&ceiling, Family::Engine, action, &Target::Resource { id }) {
                        return Err(ErrorCode::Forbidden);
                    }
                }
            }
        }
        Ok(())
    }
}
