//! Public session contract. Work text is untrusted; binding IDs resolve through
//! service-owned configuration and confer no authority by themselves.

use std::collections::BTreeSet;
use std::fmt;

use schemars::{JsonSchema, Schema, schema_for};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

pub mod codex_profile;
pub mod dispatch;
pub mod handoff;
/// Frozen Crumbles supervision wire format for compatibility fixtures only.
/// It is not the public generic session API and grants no caller authority.
pub mod legacy_supervision;
pub mod transport;

pub const VERSION: u16 = 1;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAX_SUBMISSION_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_COMMAND_BYTES: usize = 64 * 1024;
/// Shared HTTP pagination contract for sessions, artifacts and events.
pub const MAX_PAGE_SIZE: u32 = 100;
pub const DEFAULT_PAGE_SIZE: u32 = 50;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Id(
    #[serde(deserialize_with = "deserialize_id")]
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$")
    )]
    String,
);

impl Id {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value.as_bytes()[0].is_ascii_alphanumeric()
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._:/-".contains(&c));
        if !valid {
            return Err(ContractError::Invalid("identifier"));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

fn deserialize_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Id::new(String::deserialize(d)?)
        .map(|id| id.0)
        .map_err(D::Error::custom)
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct Counter(
    #[serde(deserialize_with = "deserialize_counter")]
    #[schemars(range(min = 1, max = MAX_SAFE_INTEGER))]
    u64,
);

impl Counter {
    pub fn new(value: u64) -> Result<Self, ContractError> {
        if !(1..=MAX_SAFE_INTEGER).contains(&value) {
            return Err(ContractError::Invalid("counter"));
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u64 {
        self.0
    }
}

fn deserialize_counter<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    Counter::new(u64::deserialize(d)?)
        .map(|n| n.0)
        .map_err(D::Error::custom)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceReference {
    pub system: Id,
    pub reference: Id,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Network,
    RepositoryRead,
    RepositoryWrite,
    ExternalMutation,
    HumanInput,
    SessionResume,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceBudget {
    pub wall_time_secs: Counter,
    pub cpu_time_secs: Counter,
    pub memory_bytes: Counter,
    pub max_processes: Counter,
    pub workspace_bytes: Counter,
    pub evidence_bytes: Counter,
    pub retention_secs: Counter,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkInput {
    #[schemars(length(min = 1, max = 1048576))]
    pub instructions: String,
    #[schemars(length(max = 4194304))]
    pub context: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitSession {
    /// Optional managed handoff protocol. Omission preserves legacy submissions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<handoff::ManagedHandoff>,
    #[schemars(range(min = 1, max = 1))]
    pub version: u16,
    pub idempotency_key: Id,
    pub source: SourceReference,
    pub profile_id: Id,
    pub work: WorkInput,
    pub capabilities: BTreeSet<Capability>,
    /// Service-configured capability binding IDs; never paths, commands or credentials.
    #[schemars(length(max = 32))]
    pub binding_ids: BTreeSet<Id>,
    pub budget: ResourceBudget,
}

impl SubmitSession {
    pub fn decode(bytes: &[u8]) -> Result<Self, ContractError> {
        if bytes.len() > MAX_SUBMISSION_BYTES {
            return Err(ContractError::TooLarge);
        }
        let request: Self = serde_json::from_slice(bytes)?;
        request.validate()?;
        Ok(request)
    }
    pub fn validate(&self) -> Result<(), ContractError> {
        version(self.version)?;
        if let Some(handoff) = &self.handoff {
            handoff.validate()?;
            if !self.binding_ids.contains(&handoff.binding_id)
                || !self.capabilities.contains(&Capability::ExternalMutation)
                || !self.capabilities.contains(&Capability::SessionResume)
            {
                return Err(ContractError::Invalid("managed handoff authority"));
            }
        }
        text(&self.work.instructions, 1024 * 1024, false)?;
        text(&self.work.context, 4 * 1024 * 1024, true)?;
        if self.binding_ids.len() > 32 {
            return Err(ContractError::Invalid("binding count"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Queued,
    Running,
    WaitingForInput,
    Pausing,
    Paused,
    Cancelling,
    Interrupted,
    Succeeded,
    Failed,
    Cancelled,
}

impl SessionState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectiveConfiguration {
    pub profile_revision: Counter,
    pub engine: Id,
    pub engine_version: String,
    pub capabilities: BTreeSet<Capability>,
    pub binding_revisions: Vec<BindingRevision>,
    pub budget: ResourceBudget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BindingRevision {
    pub binding_id: Id,
    pub revision: Counter,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Session {
    #[schemars(range(min = 1, max = 1))]
    pub version: u16,
    pub session_id: Id,
    pub caller_id: Id,
    pub source: SourceReference,
    pub profile_id: Id,
    pub state: SessionState,
    pub resource_version: Counter,
    pub attempt: Option<Counter>,
    pub effective: EffectiveConfiguration,
    pub created_at_ms: Counter,
    /// Null while nonterminal; a terminal deadline is frozen at settlement.
    pub retain_until_ms: Option<Counter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandRequest {
    #[schemars(range(min = 1, max = 1))]
    pub version: u16,
    pub idempotency_key: Id,
    pub expected_resource_version: Counter,
    pub action: Command,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Pause,
    Cancel,
    Resume,
    Steer {
        #[schemars(length(min = 1, max = 16384))]
        instructions: String,
    },
    Answer {
        request_id: Id,
        #[schemars(length(min = 1, max = 16384))]
        answer: String,
    },
    Approve {
        request_id: Id,
        approved: bool,
    },
}

impl CommandRequest {
    pub fn decode(bytes: &[u8]) -> Result<Self, ContractError> {
        if bytes.len() > MAX_COMMAND_BYTES {
            return Err(ContractError::TooLarge);
        }
        let request: Self = serde_json::from_slice(bytes)?;
        version(request.version)?;
        match &request.action {
            Command::Steer { instructions } => text(instructions, 16 * 1024, false)?,
            Command::Answer { answer, .. } => text(answer, 16 * 1024, false)?,
            _ => {}
        }
        Ok(request)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandReceipt {
    pub command_id: Id,
    pub session_id: Id,
    pub status: CommandStatus,
    pub resource_version: Counter,
    pub error: Option<ErrorCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Pending,
    Applied,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HumanRequest {
    pub request_id: Id,
    pub attempt: Counter,
    pub expires_at_ms: Counter,
    pub kind: HumanRequestKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HumanRequestKind {
    Question {
        question: String,
        choices: Vec<String>,
    },
    Permission {
        capability: Capability,
        binding_id: Option<Id>,
        rationale: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Event {
    #[schemars(range(min = 1, max = 1))]
    pub version: u16,
    pub session_id: Id,
    pub sequence: Counter,
    pub attempt: Option<Counter>,
    pub occurred_at_ms: Counter,
    pub payload: EventPayload,
}

impl Event {
    pub fn decode(bytes: &[u8]) -> Result<Self, ContractError> {
        if bytes.len() > 128 * 1024 {
            return Err(ContractError::TooLarge);
        }
        let event: Self = serde_json::from_slice(bytes)?;
        version(event.version)?;
        Ok(event)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventPayload {
    StateChanged {
        from: SessionState,
        to: SessionState,
        reason: String,
        resource_version: Counter,
    },
    Output {
        channel: OutputChannel,
        content: String,
    },
    ToolCall {
        call_id: Id,
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        call_id: Id,
        result: serde_json::Value,
    },
    Usage {
        cpu_time_secs: u64,
        input_tokens: u64,
        output_tokens: u64,
    },
    HumanInput {
        request: HumanRequest,
    },
    CommandUpdated {
        receipt: CommandReceipt,
    },
    ArtifactAvailable {
        artifact: Artifact,
    },
    Diagnostic {
        code: Id,
        message: String,
    },
    Result {
        result: SessionResult,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutputChannel {
    Instructions,
    Context,
    Assistant,
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub artifact_id: Id,
    pub name: String,
    pub media_type: String,
    pub bytes: u64,
    #[schemars(regex(pattern = "^[a-f0-9]{64}$"))]
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionResult {
    pub outcome: Outcome,
    pub summary: String,
    pub artifact_ids: Vec<Id>,
    pub effect_receipts: Vec<EffectReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectReceipt {
    pub operation_id: Id,
    pub binding_id: Id,
    pub status: EffectStatus,
    pub evidence: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectStatus {
    Applied,
    NotApplied,
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventPage {
    pub events: Vec<Event>,
    /// Last returned sequence, or the supplied cursor for an empty page; 0 starts a stream.
    pub next_after: u64,
    pub has_more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionPage {
    pub sessions: Vec<Session>,
    pub next_cursor: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPage {
    pub artifacts: Vec<Artifact>,
    pub next_cursor: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ErrorResponse {
    pub error: ErrorCode,
    pub message: String,
    pub request_id: Id,
    pub retryable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedVersion,
    Unauthorized,
    Forbidden,
    NotFound,
    IdempotencyConflict,
    StaleVersion,
    InvalidState,
    RequestExpired,
    CapacityUnavailable,
    EvidenceExpired,
    CursorExpired,
    Unavailable,
    Internal,
}

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("unsupported session protocol version")]
    UnsupportedVersion,
    #[error("request exceeds its byte limit")]
    TooLarge,
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("request does not match the public schema")]
    Decode(#[from] serde_json::Error),
}

fn version(value: u16) -> Result<(), ContractError> {
    if value == VERSION {
        Ok(())
    } else {
        Err(ContractError::UnsupportedVersion)
    }
}

fn text(value: &str, max: usize, empty: bool) -> Result<(), ContractError> {
    if value.len() > max {
        return Err(ContractError::TooLarge);
    }
    if (!empty && value.trim().is_empty()) || value.contains('\0') {
        return Err(ContractError::Invalid("work text"));
    }
    Ok(())
}

pub fn schemas() -> Vec<(&'static str, Schema)> {
    vec![
        ("submit-session", schema_for!(SubmitSession)),
        ("session", schema_for!(Session)),
        ("command", schema_for!(CommandRequest)),
        ("command-receipt", schema_for!(CommandReceipt)),
        ("event", schema_for!(Event)),
        ("event-page", schema_for!(EventPage)),
        ("session-page", schema_for!(SessionPage)),
        ("artifact-page", schema_for!(ArtifactPage)),
        ("artifact", schema_for!(Artifact)),
        ("result", schema_for!(SessionResult)),
        ("error", schema_for!(ErrorResponse)),
    ]
}
