//! Free-text conditions are frozen input, not executable authorization rules.
use crate::{ContractError, Id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManagedHandoff {
    pub version: u16,
    pub binding_id: Id,
    pub snapshot_id: Id,
    pub conditions: Vec<Condition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub outcome: String,
    pub source: String,
    pub revision: String,
    pub text: String,
}

/// Exact upstream action shown to the working agent before confirmation.
/// The gateway supplies the idempotency key separately; agents cannot choose it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub ticket_id: String,
    pub expected_version: crate::Counter,
    pub target_state: String,
    pub assignee_id: Option<crate::Counter>,
    pub outcome: String,
}

impl Action {
    pub fn validate(&self) -> Result<(), ContractError> {
        crate::text(&self.ticket_id, 256, false)?;
        crate::text(&self.target_state, 128, false)?;
        crate::text(&self.outcome, 32768, false)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    /// Index in the frozen, complete conditions array, not a re-numbered subset.
    pub condition_index: usize,
    pub met: bool,
    pub evidence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Confirmation {
    pub challenge_id: Id,
    /// Required even when no conditions apply: explicitly confirm the action.
    pub action_assessment: String,
    pub assessments: Vec<Assessment>,
}

impl Confirmation {
    /// Validate protocol completeness, never the truth of free-text evidence.
    pub fn all_met(&self, contract: &ManagedHandoff, outcome: &str) -> Result<bool, ContractError> {
        contract.validate()?;
        crate::text(&self.action_assessment, 8192, false)?;
        let applicable = contract.applicable(outcome);
        if self.assessments.len() != applicable.len() {
            return Err(ContractError::Invalid("handoff assessment count"));
        }
        let mut indices = std::collections::BTreeSet::new();
        let mut bytes = self.action_assessment.len();
        for assessment in &self.assessments {
            crate::text(&assessment.evidence, 8192, false)?;
            bytes += assessment.evidence.len();
            if !indices.insert(assessment.condition_index)
                || !applicable
                    .iter()
                    .any(|(index, _)| *index == assessment.condition_index)
            {
                return Err(ContractError::Invalid("handoff assessment scope"));
            }
        }
        if bytes > 65536 {
            return Err(ContractError::Invalid("handoff assessment size"));
        }
        Ok(self.assessments.iter().all(|a| a.met))
    }
}

impl ManagedHandoff {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.version != 1 || self.conditions.len() > 100 {
            return Err(ContractError::Invalid("handoff contract"));
        }
        let mut bytes = 0;
        for condition in &self.conditions {
            crate::text(&condition.outcome, 128, false)?;
            crate::text(&condition.source, 256, false)?;
            crate::text(&condition.revision, 128, false)?;
            crate::text(&condition.text, 65536, false)?;
            bytes += condition.text.len();
        }
        if bytes > 262144 {
            return Err(ContractError::Invalid("handoff conditions size"));
        }
        Ok(())
    }

    pub fn applicable(&self, outcome: &str) -> Vec<(usize, &Condition)> {
        self.conditions
            .iter()
            .enumerate()
            .filter(|(_, c)| c.outcome == outcome)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crumbles_contract_preserves_outcome_applicability_and_empty_confirmation() {
        let mut input: ManagedHandoff = serde_json::from_value(serde_json::json!({
            "version":1,"binding_id":"crumbles-handoff","snapshot_id":"dispatch-123",
            "conditions":[{"outcome":"Review","source":"policy:junior:common","revision":"2","text":"CI green"},
              {"outcome":"Blocked","source":"policy:junior:common","revision":"2","text":"Explain blocker"}]
        })).unwrap();
        input.validate().unwrap();
        assert_eq!(input.applicable("Blocked")[0].1.text, "Explain blocker");
        input.conditions.clear();
        input.validate().unwrap();
        input.version = 2;
        assert!(input.validate().is_err());
    }

    #[test]
    fn assessments_must_cover_each_applicable_condition_exactly_once() {
        let contract: ManagedHandoff = serde_json::from_value(serde_json::json!({
            "version":1,"binding_id":"crumbles","snapshot_id":"dispatch-1",
            "conditions":[{"outcome":"Review","source":"policy","revision":"1","text":"Tests pass"},
              {"outcome":"Review","source":"type","revision":"1","text":"Reproducer added"},
              {"outcome":"Blocked","source":"policy","revision":"1","text":"Explain blocker"}]
        }))
        .unwrap();
        let mut confirmation = Confirmation {
            challenge_id: Id::new("challenge-1").unwrap(),
            action_assessment: "Checked action".into(),
            assessments: vec![
                Assessment {
                    condition_index: 0,
                    met: true,
                    evidence: "Test output".into(),
                },
                Assessment {
                    condition_index: 0,
                    met: true,
                    evidence: "Duplicate".into(),
                },
            ],
        };
        assert!(confirmation.all_met(&contract, "Review").is_err());
        confirmation.assessments[1].condition_index = 2;
        assert!(confirmation.all_met(&contract, "Review").is_err());
        confirmation.assessments[1].condition_index = 1;
        assert!(confirmation.all_met(&contract, "Review").unwrap());
        confirmation.assessments[1].met = false;
        assert!(!confirmation.all_met(&contract, "Review").unwrap());
        confirmation.assessments[1].evidence = " ".into();
        assert!(confirmation.all_met(&contract, "Review").is_err());
        confirmation.assessments.pop();
        assert!(confirmation.all_met(&contract, "Review").is_err());
    }
}
