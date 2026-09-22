//! Trusted Runner control wire. These envelopes are accepted only through the
//! authenticated broker, never from session instructions or public callers.
use crate::{Counter, EventPayload, HumanRequest, Id, Session, SubmitSession};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_TRANSFER_BYTES: usize = 12 * 1024 * 1024;
pub const PART_BYTES: usize = 24 * 1024;
pub const PERMIT_TTL_MS: u64 = 2000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_profile: Option<CodexCredentialLease>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_access: Option<ManagedAccess>,
    pub assignment_id: Id,
    pub generation: Counter,
    pub runner_revision: Counter,
    pub session: Session,
    pub request: SubmitSession,
    pub human_context: String,
    pub resume: Option<serde_json::Value>,
}
/// Private, exclusive credential lease. Never part of the public session model.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexCredentialLease {
    pub settings: crate::codex_profile::CodexSettings,
    pub epoch: Counter,
    pub auth_json: String,
}
/// Attempt-scoped gateway access, carried only in the encrypted broker envelope.
/// This is not the upstream workflow credential and must not enter model evidence.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedAccess {
    pub url: String,
    pub bearer_token: String,
    pub execution_deadline_ms: Counter,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub transfer_id: Id,
    pub index: u32,
    pub count: u32,
    pub sha256: String,
    pub data: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    EffectResponse {
        request_id: Id,
        accepted: bool,
        permit: Option<Counter>,
        generation: Option<Counter>,
        status: Option<crate::EffectStatus>,
    },
    ReleaseWorkspace {
        session_id: Id,
        revision: Counter,
    },
    AssignmentPart {
        part: Part,
    },
    Permit {
        assignment_id: Id,
        nonce: Id,
        allowed: bool,
        ttl_ms: u64,
    },
    Retention {
        session_id: Id,
        revision: Counter,
        retain_until_ms: Counter,
        pinned: bool,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Report {
    /// Observational cleanup failure. Never authorizes release or changes capacity.
    WorkspaceCleanupFailed {
        session_id: Id,
        revision: Counter,
        reason: CleanupFailure,
    },
    CodexTemplates {
        generation: Counter,
        templates: std::collections::BTreeSet<Id>,
    },
    WorkspaceReleased {
        session_id: Id,
        revision: Counter,
    },
    PermitRequest {
        assignment_id: Id,
        nonce: Id,
    },
    EvidencePart {
        part: Part,
    },
}

/// Closed categories only: host paths, helper responses and secrets stay local.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupFailure {
    HelperRefused,
    StorageUnavailable,
    Protected,
    CleanupUnavailable,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub assignment_id: Id,
    pub payload: EvidencePayload,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidencePayload {
    /// Trusted executor output, encrypted in transport; never journaled as
    /// model evidence or accepted through the public API.
    CodexCredentialReturn {
        epoch: Counter,
        auth_json: String,
    },
    Effect {
        request_id: Id,
        operation_id: Id,
        binding_id: Id,
        fingerprint: String,
        action: EffectAction,
    },
    Event {
        event: EventPayload,
    },
    Checkpoint {
        resume: serde_json::Value,
    },
    Human {
        request: HumanRequest,
    },
    Artifact {
        artifact_id: Id,
        name: String,
        media_type: String,
        base64: String,
    },
    Finished {
        cleaned: bool,
        interrupted: bool,
        outcome: crate::Outcome,
        summary: String,
    },
}

pub fn split(id: Id, bytes: &[u8]) -> Result<Vec<Part>, &'static str> {
    if bytes.is_empty() || bytes.len() > MAX_TRANSFER_BYTES {
        return Err("transfer_size");
    }
    let count = bytes.len().div_ceil(PART_BYTES) as u32;
    let sha256 = hex::encode(Sha256::digest(bytes));
    Ok(bytes
        .chunks(PART_BYTES)
        .enumerate()
        .map(|(index, bytes)| Part {
            transfer_id: id.clone(),
            index: index as u32,
            count,
            sha256: sha256.clone(),
            data: STANDARD.encode(bytes),
        })
        .collect())
}
pub fn assemble(parts: &[Part]) -> Result<Option<Vec<u8>>, &'static str> {
    let Some(first) = parts.first() else {
        return Ok(None);
    };
    if first.count == 0
        || first.count as usize > MAX_TRANSFER_BYTES.div_ceil(PART_BYTES)
        || first.sha256.len() != 64
        || !first.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("transfer_header");
    }
    let mut ordered = std::collections::BTreeMap::new();
    for part in parts {
        if part.transfer_id != first.transfer_id
            || part.count != first.count
            || part.sha256 != first.sha256
            || part.index >= first.count
            || part.data.len() > PART_BYTES.div_ceil(3) * 4
        {
            return Err("transfer_binding");
        }
        if let Some(old) = ordered.insert(part.index, &part.data)
            && old != &part.data
        {
            return Err("transfer_conflict");
        }
    }
    if ordered.len() != first.count as usize {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    for data in ordered.values() {
        let part = STANDARD.decode(data).map_err(|_| "transfer_encoding")?;
        if part.is_empty() || part.len() > PART_BYTES {
            return Err("transfer_size");
        }
        bytes.extend(part);
    }
    if bytes.len() > MAX_TRANSFER_BYTES || hex::encode(Sha256::digest(&bytes)) != first.sha256 {
        return Err("transfer_digest");
    }
    Ok(Some(bytes))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfers_are_bounded_complete_and_bound_to_exact_content() {
        let original = vec![42; PART_BYTES + 7];
        let parts = split(Id::new("test").unwrap(), &original).unwrap();
        assert!(assemble(&parts[..1]).unwrap().is_none());
        assert_eq!(assemble(&parts).unwrap().unwrap(), original);
        let mut wrong = parts.clone();
        wrong[1].sha256 = "00".repeat(32);
        assert!(assemble(&wrong).is_err());
        let mut wrong = parts.clone();
        wrong[1].data = STANDARD.encode(b"changed");
        assert!(assemble(&wrong).is_err());
        let mut wrong = parts;
        wrong[0].count = u32::MAX;
        assert!(assemble(&wrong).is_err());
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectAction {
    Intent,
    Inspect,
    Reconcile {
        generation: Counter,
        receipt: crate::EffectReceipt,
    },
}
