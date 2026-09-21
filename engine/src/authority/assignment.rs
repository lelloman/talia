use super::*;
use crate::delivery::{Assigned, Assignment};
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignmentRequest {
    pub request_id: String,
    pub client_id: String,
    pub slot_id: Option<String>,
    #[serde(default)]
    pub client_default: bool,
    pub expected_revision: u64,
    pub assignment: Assignment,
}
impl Store {
    pub fn agent_assignment_set(
        &mut self,
        session: &Session,
        r: &AssignmentRequest,
        now: i64,
    ) -> Result<Value> {
        if r.client_default == r.slot_id.is_some() {
            return Err(ErrorCode::InvalidInput);
        }
        let target = Target::Assignment {
            client_id: r.client_id.clone(),
            slot_id: r.slot_id.clone(),
        };
        let before = if self
            .agent_require(session, Operation::AssignmentSet, &target)
            .is_ok()
        {
            self.assignment_get(&r.client_id, r.slot_id.as_deref())
                .ok()
                .map(|a| Revision {
                    target: target.clone(),
                    revision: a.revision.to_string(),
                })
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let admission = self.agent_admit(
            session,
            &r.request_id,
            Operation::AssignmentSet,
            &[target.clone()],
            &serde_json::to_value(r)?,
            &before,
            now,
        )?;
        let Some(permit) = admission.permit else {
            return Ok(assignment_result(admission.record, None));
        };
        self.agent_start(&permit, now)?;
        self.conn
            .execute_batch("SAVEPOINT agent_assignment_effect")?;
        let result = (|| {
            self.agent_check_permit(&permit)?;
            self.agent_require(
                session,
                Operation::DefinitionsRead,
                &Target::Definition {
                    key: catalog::Key::new("dashboard", &r.assignment.dashboard_id),
                },
            )?;
            let assignment = self.assignment_set(
                &r.client_id,
                r.slot_id.as_deref(),
                r.expected_revision,
                &r.assignment,
            )?;
            let audit = self.agent_finish(
                &permit,
                Status::Complete,
                None,
                &[Revision {
                    target,
                    revision: assignment.revision.to_string(),
                }],
                now,
            )?;
            Ok(assignment_result(audit, Some(assignment)))
        })();
        match result {
            Ok(value) => {
                self.conn.execute_batch("RELEASE agent_assignment_effect")?;
                Ok(value)
            }
            Err(code) => {
                self.conn.execute_batch(
                    "ROLLBACK TO agent_assignment_effect; RELEASE agent_assignment_effect",
                )?;
                let audit = self.agent_finish(&permit, Status::Failed, Some(code), &[], now)?;
                Ok(assignment_result(audit, None))
            }
        }
    }
}
fn assignment_result(audit: AuditRecord, assignment: Option<Assigned>) -> Value {
    let failed = audit.status != Status::Complete;
    let error = audit.error.unwrap_or(ErrorCode::Unknown);
    let mut value = json!({"audit":audit,"assignment":assignment});
    if failed {
        value["error"] = json!(error);
    }
    value
}
