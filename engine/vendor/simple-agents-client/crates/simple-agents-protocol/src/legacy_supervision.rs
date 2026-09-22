//! Preserved from Crumbles 10c5acf9bc91c83eb76774e5f26a11fb00080285. MIT, (c) 2026 lelloman.
//! Versioned application protocol; contains no sockets, storage or credentials.
//! Authenticate transport identities separately, then decode, validate and fence
//! every command against live Runner authority before acting on it.
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 128 * 1024;
pub const MAX_CONTENT_BYTES: usize = 64 * 1024;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
/// All integers on the wire are exact in JavaScript as well as Kotlin/Rust.
pub const MAX_SEQUENCE: u64 = 9_007_199_254_740_991;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("unsupported supervision protocol")]
    Version,
    #[error("invalid supervision message")]
    Invalid,
    #[error("supervision message limit exceeded")]
    Limit,
    #[error("credential boundary violation")]
    Credential,
    #[error("stale supervision precondition")]
    Stale,
    #[error("supervision command expired")]
    Expired,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "frame", rename_all = "snake_case", deny_unknown_fields)]
pub enum Frame {
    Hello {
        versions: Vec<u16>,
        runner_id: String,
        workspace_id: i64,
        nonce: String,
        events_acked: u64,
        commands_acked: u64,
    },
    Welcome {
        version: u16,
        runner_id: String,
        nonce: String,
        connection_generation: u64,
        events_acked: u64,
        commands_acked: u64,
    },
    Data {
        connection_generation: u64,
        envelope: Box<Envelope>,
    },
    Ack {
        version: u16,
        connection_generation: u64,
        stream: Stream,
        through: u64,
    },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    RunnerEvents,
    HumanCommands,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u16,
    pub message_id: String,
    pub stream: Stream,
    pub sequence: u64,
    pub runner_id: String,
    pub workspace_id: i64,
    pub target: Target,
    pub actor: Actor,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub payload: Payload,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Runner,
    Profile {
        profile_id: i64,
    },
    Session {
        profile_id: i64,
        ticket_id: String,
        run_id: String,
        attempt: u32,
        session_generation: u64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Actor {
    Runner,
    Profile {
        profile_id: i64,
        crumbles_user_id: i64,
    },
    Human {
        user_id: i64,
        client_id: String,
    },
    Server,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Active,
    WaitingInput,
    Paused,
    Interrupted,
    Draining,
    Incompatible,
    Terminal,
    Offline,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    View,
    Operate,
    Administer,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Network,
    RepositoryWrite,
    ExternalMutation,
    Clarification,
    SessionResume,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptKind {
    Prompt,
    Response,
    ToolCall,
    ToolResult,
    Stdout,
    Stderr,
    Lifecycle,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Precondition {
    pub state: State,
    pub resource_version: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Pause,
    Resume,
    Cancel,
    Retry,
    Spawn,
    ClarificationReply {
        request_id: String,
        text: String,
    },
    PermissionReply {
        request_id: String,
        permission: Permission,
        approved: bool,
    },
    Steering {
        text: String,
    },
    Followup {
        text: String,
    },
    Revoke {
        user_id: i64,
    },
    Grant {
        user_id: i64,
        capability: Capability,
    },
    EmergencyStop {
        message: String,
        deadline: DateTime<Utc>,
    },
    Diagnostics {
        include_transcript: bool,
    },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandState {
    Queued,
    Delivered,
    Acknowledged,
    Expired,
    Rejected,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Payload {
    Lifecycle {
        state: State,
        resource_version: u64,
        reason: String,
    },
    Transcript {
        journal_sequence: u64,
        event_id: String,
        event_kind: TranscriptKind,
        sha256: String,
        bytes: u64,
        media_type: String,
    },
    Artifact {
        sha256: String,
        bytes: u64,
        media_type: String,
        name: String,
    },
    ArtifactChunk {
        sha256: String,
        offset: u64,
        content_base64: String,
    },
    Question {
        request_id: String,
        question: String,
        choices: Vec<String>,
    },
    PermissionRequest {
        request_id: String,
        permission: Permission,
        rationale: String,
    },
    Publication {
        operation_id: String,
        step: String,
        generation: u64,
        branch: Option<String>,
        head: Option<String>,
        pull_request_id: Option<String>,
        updated_at: DateTime<Utc>,
    },
    Command {
        expected: Precondition,
        action: Command,
    },
    CommandResult {
        message_id: String,
        state: CommandState,
        reason: String,
        resource_version: u64,
    },
    Grant {
        user_id: i64,
        capability: Capability,
        acl_version: u64,
        valid_until: DateTime<Utc>,
    },
    RevokeGrant {
        user_id: i64,
        acl_version: u64,
    },
    Retention {
        pinned: bool,
        retain_until: Option<DateTime<Utc>>,
        max_bytes: Option<u64>,
    },
    DeleteHistory {
        through_journal_sequence: u64,
    },
}

fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:/".contains(&c))
}
fn text(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.contains('\0')
}
fn counter(n: u64) -> bool {
    n > 0 && n <= MAX_SEQUENCE
}
fn user(n: i64) -> bool {
    n > 0 && n <= MAX_SEQUENCE as i64
}
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Command {
    pub fn capability(&self) -> Capability {
        match self {
            Self::Grant { .. }
            | Self::Revoke { .. }
            | Self::EmergencyStop { .. }
            | Self::Diagnostics {
                include_transcript: true,
            } => Capability::Administer,
            Self::Diagnostics {
                include_transcript: false,
            } => Capability::View,
            _ => Capability::Operate,
        }
    }
    pub fn durable_until_ack(&self) -> bool {
        matches!(
            self,
            Self::Cancel | Self::Revoke { .. } | Self::EmergencyStop { .. }
        )
    }
    pub fn max_lifetime_secs(&self) -> i64 {
        match self {
            Self::PermissionReply { .. } | Self::Spawn => 300,
            Self::Steering { .. } => 900,
            Self::ClarificationReply { .. } | Self::Followup { .. } => 86400,
            _ => 3600,
        }
    }
    fn validate(&self, target: &Target) -> bool {
        let session = matches!(target, Target::Session { .. });
        match self {
            Self::Spawn => matches!(target, Target::Profile { .. }),
            Self::Pause | Self::Resume | Self::Cancel | Self::Retry => session,
            Self::ClarificationReply {
                request_id,
                text: value,
            } => session && id(request_id) && text(value, MAX_TEXT_BYTES),
            Self::PermissionReply { request_id, .. } => session && id(request_id),
            Self::Steering { text: value } | Self::Followup { text: value } => {
                session && text(value, MAX_TEXT_BYTES)
            }
            Self::Revoke { user_id } | Self::Grant { user_id, .. } => {
                matches!(target, Target::Runner) && user(*user_id)
            }
            Self::EmergencyStop { message, .. } => {
                matches!(target, Target::Runner) && text(message, MAX_TEXT_BYTES)
            }
            Self::Diagnostics { .. } => matches!(target, Target::Runner | Target::Session { .. }),
        }
    }
}
impl Envelope {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != VERSION {
            return Err(Error::Version);
        }
        if !id(&self.message_id)
            || !id(&self.runner_id)
            || !user(self.workspace_id)
            || !counter(self.sequence)
        {
            return Err(Error::Invalid);
        }
        match &self.target {
            Target::Runner => {}
            Target::Profile { profile_id } if user(*profile_id) => {}
            Target::Session {
                profile_id,
                ticket_id,
                run_id,
                attempt,
                session_generation,
            } if user(*profile_id)
                && id(ticket_id)
                && id(run_id)
                && *attempt > 0
                && counter(*session_generation) => {}
            _ => return Err(Error::Invalid),
        }
        match &self.actor {
            Actor::Human { user_id, client_id } if user(*user_id) && id(client_id) => {}
            Actor::Profile {
                profile_id,
                crumbles_user_id,
            } if user(*profile_id)
                && user(*crumbles_user_id)
                && matches!(&self.target,Target::Session{profile_id:p,..}|Target::Profile{profile_id:p} if p==profile_id) =>
                {}
            Actor::Runner | Actor::Server => {}
            _ => return Err(Error::Invalid),
        }
        let is_command = matches!(self.payload, Payload::Command { .. });
        if is_command != (self.stream == Stream::HumanCommands)
            || is_command && !matches!(self.actor, Actor::Human { .. } | Actor::Server)
            || !is_command && !matches!(self.actor, Actor::Runner | Actor::Profile { .. })
        {
            return Err(Error::Invalid);
        }
        if matches!(self.actor, Actor::Server)
            && !matches!(
                self.payload,
                Payload::Command {
                    action: Command::EmergencyStop { .. },
                    ..
                }
            )
        {
            return Err(Error::Invalid);
        }
        if self.expires_at.is_some_and(|v| v <= self.created_at) {
            return Err(Error::Invalid);
        }
        let session = matches!(self.target, Target::Session { .. });
        let valid = match &self.payload {
            Payload::Lifecycle {
                resource_version,
                reason,
                ..
            } => counter(*resource_version) && id(reason),
            Payload::Transcript {
                journal_sequence,
                event_id,
                sha256,
                bytes,
                media_type,
                ..
            } => {
                session
                    && counter(*journal_sequence)
                    && id(event_id)
                    && hash(sha256)
                    && *bytes <= MAX_SEQUENCE
                    && text(media_type, 128)
            }
            Payload::Artifact {
                sha256,
                bytes,
                media_type,
                name,
            } => {
                session
                    && hash(sha256)
                    && *bytes <= MAX_SEQUENCE
                    && text(media_type, 128)
                    && text(name, 256)
            }
            Payload::ArtifactChunk {
                sha256,
                offset,
                content_base64,
            } => {
                session
                    && hash(sha256)
                    && *offset <= MAX_SEQUENCE
                    && content_base64.len() <= MAX_CONTENT_BYTES * 4 / 3 + 4
                    && STANDARD
                        .decode(content_base64)
                        .is_ok_and(|b| !b.is_empty() && b.len() <= MAX_CONTENT_BYTES)
            }
            Payload::Question {
                request_id,
                question,
                choices,
            } => {
                session
                    && id(request_id)
                    && text(question, MAX_TEXT_BYTES)
                    && choices.len() <= 16
                    && choices.iter().all(|s| text(s, 1024))
            }
            Payload::PermissionRequest {
                request_id,
                rationale,
                ..
            } => session && id(request_id) && text(rationale, MAX_TEXT_BYTES),
            Payload::Publication {
                operation_id,
                step,
                generation,
                branch,
                head,
                pull_request_id,
                ..
            } => {
                session
                    && id(operation_id)
                    && id(step)
                    && counter(*generation)
                    && branch.as_ref().is_none_or(|value| text(value, 512))
                    && head.as_ref().is_none_or(|value| id(value))
                    && pull_request_id.as_ref().is_none_or(|value| id(value))
            }
            Payload::Command { expected, action } => {
                let expiry = if action.durable_until_ack() {
                    self.expires_at.is_none()
                } else {
                    self.expires_at.is_some_and(|t| {
                        (t - self.created_at).num_seconds() <= action.max_lifetime_secs()
                    })
                };
                counter(expected.resource_version) && action.validate(&self.target) && expiry
            }
            Payload::CommandResult {
                message_id,
                reason,
                resource_version,
                ..
            } => id(message_id) && id(reason) && counter(*resource_version),
            Payload::Grant {
                user_id,
                acl_version,
                valid_until,
                ..
            } => {
                matches!(self.target, Target::Runner)
                    && user(*user_id)
                    && counter(*acl_version)
                    && *valid_until > self.created_at
                    && (*valid_until - self.created_at).num_seconds() <= 300
            }
            Payload::RevokeGrant {
                user_id,
                acl_version,
            } => matches!(self.target, Target::Runner) && user(*user_id) && counter(*acl_version),
            Payload::Retention { max_bytes, .. } => session && max_bytes.is_none_or(counter),
            Payload::DeleteHistory {
                through_journal_sequence,
            } => session && *through_journal_sequence <= MAX_SEQUENCE,
        };
        if !valid {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    /// A transport generation fences connections; this separately fences the
    /// durable target generation and live resource state at execution time.
    pub fn revalidate_command(
        &self,
        runner: &str,
        workspace: i64,
        target: &Target,
        current: &Precondition,
        now: DateTime<Utc>,
    ) -> Result<(), Error> {
        self.validate()?;
        let Payload::Command { expected, .. } = &self.payload else {
            return Err(Error::Invalid);
        };
        if self.runner_id != runner
            || self.workspace_id != workspace
            || &self.target != target
            || expected != current
        {
            return Err(Error::Stale);
        }
        if self.expires_at.is_some_and(|v| v <= now) {
            return Err(Error::Expired);
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> Result<String, Error> {
        self.validate()?;
        Ok(hex::encode(Sha256::digest(
            serde_json::to_vec(self).map_err(|_| Error::Invalid)?,
        )))
    }
}

impl Frame {
    pub fn validate(&self) -> Result<(), Error> {
        let valid = match self {
            Self::Hello {
                versions,
                runner_id,
                workspace_id,
                nonce,
                events_acked,
                commands_acked,
            } => {
                !versions.is_empty()
                    && versions.len() <= 8
                    && versions.iter().all(|v| *v > 0)
                    && id(runner_id)
                    && user(*workspace_id)
                    && id(nonce)
                    && *events_acked <= MAX_SEQUENCE
                    && *commands_acked <= MAX_SEQUENCE
            }
            Self::Welcome {
                version,
                runner_id,
                nonce,
                connection_generation,
                events_acked,
                commands_acked,
            } => {
                *version == VERSION
                    && id(runner_id)
                    && id(nonce)
                    && counter(*connection_generation)
                    && *events_acked <= MAX_SEQUENCE
                    && *commands_acked <= MAX_SEQUENCE
            }
            Self::Data {
                connection_generation,
                envelope,
            } => {
                envelope.validate()?;
                counter(*connection_generation)
            }
            Self::Ack {
                version,
                connection_generation,
                through,
                ..
            } => *version == VERSION && counter(*connection_generation) && *through <= MAX_SEQUENCE,
        };
        if valid { Ok(()) } else { Err(Error::Invalid) }
    }
}
pub fn negotiate(offered: &[u16]) -> Result<u16, Error> {
    if offered.len() <= 8 && offered.contains(&VERSION) {
        Ok(VERSION)
    } else {
        Err(Error::Version)
    }
}

/// Only the host-side capture boundary receives resolved values. They are never
/// fields of a message, serialized configuration, Debug output or wire schema.
#[derive(Default)]
pub struct CredentialBoundary {
    forbidden: Vec<Vec<u8>>,
}
impl CredentialBoundary {
    pub fn new(values: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            forbidden: values.into_iter().filter(|v| !v.is_empty()).collect(),
        }
    }
    fn accepts(&self, bytes: &[u8]) -> bool {
        !bytes
            .windows(b"/run/secrets/".len())
            .any(|w| w == b"/run/secrets/")
            && self
                .forbidden
                .iter()
                .all(|v| !bytes.windows(v.len()).any(|w| w == v))
    }
    fn value(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => self.accepts(s.as_bytes()),
            serde_json::Value::Array(a) => a.iter().all(|v| self.value(v)),
            serde_json::Value::Object(o) => o
                .iter()
                .all(|(k, v)| self.accepts(k.as_bytes()) && self.value(v)),
            _ => true,
        }
    }
    fn inspect(&self, frame: &Frame) -> Result<(), Error> {
        let value = serde_json::to_value(frame).map_err(|_| Error::Invalid)?;
        if !self.value(&value) {
            return Err(Error::Credential);
        }
        if let Frame::Data { envelope, .. } = frame
            && let Payload::ArtifactChunk { content_base64, .. } = &envelope.payload
            && !self.accepts(
                &STANDARD
                    .decode(content_base64)
                    .map_err(|_| Error::Invalid)?,
            )
        {
            return Err(Error::Credential);
        }
        Ok(())
    }
    pub fn encode(&self, frame: &Frame) -> Result<Vec<u8>, Error> {
        frame.validate()?;
        self.inspect(frame)?;
        let bytes = serde_json::to_vec(frame).map_err(|_| Error::Invalid)?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(Error::Limit);
        }
        Ok(bytes)
    }
    pub fn decode(&self, bytes: &[u8]) -> Result<Frame, Error> {
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(Error::Limit);
        }
        let frame: Frame = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        // Some internally tagged unit variants otherwise ignore extra fields.
        // Structural round-trip equality also requires explicit nullable fields
        // and canonical UTC timestamps, preventing cross-client interpretation.
        let wire: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        if serde_json::to_value(&frame).map_err(|_| Error::Invalid)? != wire {
            return Err(Error::Invalid);
        }
        frame.validate()?;
        self.inspect(&frame)?;
        Ok(frame)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeliveryPosition {
    Next,
    Duplicate,
    Gap,
}
/// Duplicate classification is not permission to accept changed bytes: compare
/// message ID + fingerprint against the durable inbox before acknowledging it.
pub fn delivery_position(through: u64, sequence: u64) -> Result<DeliveryPosition, Error> {
    if through > MAX_SEQUENCE || !counter(sequence) {
        return Err(Error::Invalid);
    }
    Ok(if sequence <= through {
        DeliveryPosition::Duplicate
    } else if sequence == through + 1 {
        DeliveryPosition::Next
    } else {
        DeliveryPosition::Gap
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn envelope() -> Envelope {
        Envelope {
            version: VERSION,
            message_id: "command-1".into(),
            stream: Stream::HumanCommands,
            sequence: 1,
            runner_id: "runner-1".into(),
            workspace_id: 1,
            target: Target::Session {
                profile_id: 2,
                ticket_id: "LLPR/CRUMBLES-1".into(),
                run_id: "run-1".into(),
                attempt: 1,
                session_generation: 1,
            },
            actor: Actor::Human {
                user_id: 3,
                client_id: "web-1".into(),
            },
            created_at: "2026-09-07T12:00:00Z".parse().unwrap(),
            expires_at: Some("2026-09-07T12:01:00Z".parse().unwrap()),
            payload: Payload::Command {
                expected: Precondition {
                    state: State::Active,
                    resource_version: 1,
                },
                action: Command::Pause,
            },
        }
    }
    #[test]
    fn round_trip_strict_schema_and_transport_independent_fingerprint() {
        let envelope = envelope();
        let frame = Frame::Data {
            connection_generation: 1,
            envelope: Box::new(envelope.clone()),
        };
        let boundary = CredentialBoundary::default();
        let fixture = include_bytes!("../../../contracts/legacy/runner-supervision-v1.json");
        assert_eq!(boundary.decode(fixture).unwrap(), frame);
        assert_eq!(
            boundary.decode(&boundary.encode(&frame).unwrap()).unwrap(),
            frame
        );
        let mut wire = serde_json::to_value(&frame).unwrap();
        wire["envelope"]["payload"]["action"]["shell"] = "rm -rf /".into();
        assert_eq!(
            boundary.decode(&serde_json::to_vec(&wire).unwrap()),
            Err(Error::Invalid)
        );
        assert_eq!(
            envelope.fingerprint().unwrap(),
            envelope.clone().fingerprint().unwrap()
        );
        assert_eq!(negotiate(&[9, 1]).unwrap(), 1);
        assert_eq!(negotiate(&[9]), Err(Error::Version));
    }
    #[test]
    fn generation_state_identity_expiry_and_scope_are_fenced() {
        let envelope = envelope();
        let expected = Precondition {
            state: State::Active,
            resource_version: 1,
        };
        assert!(
            envelope
                .revalidate_command(
                    "runner-1",
                    1,
                    &envelope.target,
                    &expected,
                    envelope.created_at
                )
                .is_ok()
        );
        assert_eq!(
            envelope.revalidate_command(
                "runner-2",
                1,
                &envelope.target,
                &expected,
                envelope.created_at
            ),
            Err(Error::Stale)
        );
        assert_eq!(
            envelope.revalidate_command(
                "runner-1",
                1,
                &envelope.target,
                &expected,
                envelope.expires_at.unwrap()
            ),
            Err(Error::Expired)
        );
        let mut wrong = envelope.clone();
        wrong.target = Target::Runner;
        assert!(wrong.validate().is_err());
        let mut old = envelope.target.clone();
        if let Target::Session {
            session_generation, ..
        } = &mut old
        {
            *session_generation += 1;
        }
        assert_eq!(
            envelope.revalidate_command("runner-1", 1, &old, &expected, envelope.created_at),
            Err(Error::Stale)
        );
        let mut cancel = envelope;
        if let Payload::Command { action, .. } = &mut cancel.payload {
            *action = Command::Cancel;
        }
        assert!(cancel.validate().is_err());
        cancel.expires_at = None;
        assert!(cancel.validate().is_ok());
    }
    #[test]
    fn bounded_frames_replay_and_secret_capture_boundary() {
        let mut envelope = envelope();
        envelope.payload = Payload::Command {
            expected: Precondition {
                state: State::Active,
                resource_version: 1,
            },
            action: Command::Followup {
                text: "literal SECRET_SENTINEL".into(),
            },
        };
        let boundary = CredentialBoundary::new([b"SECRET_SENTINEL".to_vec()]);
        assert_eq!(
            boundary.encode(&Frame::Data {
                connection_generation: 1,
                envelope: Box::new(envelope.clone())
            }),
            Err(Error::Credential)
        );
        envelope.stream = Stream::RunnerEvents;
        envelope.actor = Actor::Runner;
        envelope.expires_at = None;
        envelope.payload = Payload::ArtifactChunk {
            sha256: "a".repeat(64),
            offset: 0,
            content_base64: STANDARD.encode(b"SECRET_SENTINEL"),
        };
        assert_eq!(
            boundary.encode(&Frame::Data {
                connection_generation: 1,
                envelope: Box::new(envelope)
            }),
            Err(Error::Credential)
        );
        assert_eq!(
            boundary.decode(&vec![b' '; MAX_FRAME_BYTES + 1]),
            Err(Error::Limit)
        );
        assert_eq!(delivery_position(1, 1), Ok(DeliveryPosition::Duplicate));
        assert_eq!(delivery_position(1, 2), Ok(DeliveryPosition::Next));
        assert_eq!(delivery_position(1, 3), Ok(DeliveryPosition::Gap));
    }
}
