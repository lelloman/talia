//! Trusted agent boundary. Guest code cannot construct sessions or dispatch permits.
use crate::{catalog, definitions::identifier, store::Store};
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Authoring,
    Alerts,
    Engine,
    Live,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Read,
    Acknowledge,
    Silence,
    Configure,
    Observe,
    Register,
    List,
    Validate,
    Save,
    Assign,
    History,
    Subscribe,
    Write,
    Set,
    Run,
    RunStatus,
    CancelRun,
    ResumeWatch,
    Inspect,
    Execute,
    Reload,
    Audit,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    All,
    Definition {
        definition_kind: String,
        #[serde(default)]
        id: Option<String>,
    },
    Resource {
        id: String,
    },
    Client {
        client_id: String,
        #[serde(default)]
        slot_id: Option<String>,
        #[serde(default)]
        instance_id: Option<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub family: Family,
    pub actions: BTreeSet<Action>,
    pub scope: Scope,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Definition {
        key: catalog::Key,
    },
    Assignment {
        client_id: String,
        slot_id: Option<String>,
    },
    Resource {
        id: String,
    },
    Client {
        client_id: String,
    },
    Live {
        client_id: String,
        slot_id: String,
        instance_id: String,
    },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    DefinitionsRead,
    DefinitionsList,
    DefinitionsValidate,
    DefinitionsSave,
    AssignmentSet,
    EngineRead,
    EngineHistory,
    EngineSubscribe,
    EngineWrite,
    EngineSet,
    EngineRun,
    EngineRunStatus,
    EngineCancelRun,
    EngineResumeWatch,
    ClientsList,
    LiveInspect,
    LiveExecute,
    LiveReload,
}
impl Operation {
    fn permission(self) -> (Family, Action) {
        use Action as A;
        use Family as F;
        match self {
            Self::DefinitionsRead => (F::Authoring, A::Read),
            Self::DefinitionsList => (F::Authoring, A::List),
            Self::DefinitionsValidate => (F::Authoring, A::Validate),
            Self::DefinitionsSave => (F::Authoring, A::Save),
            Self::AssignmentSet => (F::Authoring, A::Assign),
            Self::EngineRead => (F::Engine, A::Read),
            Self::EngineHistory => (F::Engine, A::History),
            Self::EngineSubscribe => (F::Engine, A::Subscribe),
            Self::EngineWrite => (F::Engine, A::Write),
            Self::EngineSet => (F::Engine, A::Set),
            Self::EngineRun => (F::Engine, A::Run),
            Self::EngineRunStatus => (F::Engine, A::RunStatus),
            Self::EngineCancelRun => (F::Engine, A::CancelRun),
            Self::EngineResumeWatch => (F::Engine, A::ResumeWatch),
            Self::ClientsList => (F::Live, A::List),
            Self::LiveInspect => (F::Live, A::Inspect),
            Self::LiveExecute => (F::Live, A::Execute),
            Self::LiveReload => (F::Live, A::Reload),
        }
    }
    fn accepts(self, t: &Target) -> bool {
        match self {
            Self::AssignmentSet => matches!(t, Target::Assignment { .. }),
            Self::ClientsList => matches!(t, Target::Client { .. } | Target::Live { .. }),
            Self::LiveInspect | Self::LiveExecute | Self::LiveReload => {
                matches!(t, Target::Live { .. })
            }
            _ => match self.permission().0 {
                Family::Authoring => matches!(t, Target::Definition { .. }),
                Family::Engine | Family::Alerts => matches!(t, Target::Resource { .. }),
                Family::Live => false,
            },
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthenticated,
    Forbidden,
    InvalidInput,
    Conflict,
    NotFound,
    ValidationFailed,
    StorageError,
    Unknown,
    Cancelled,
    TimedOut,
    TargetUnavailable,
    StaleInstance,
    DirtyAckRequired,
    LimitExceeded,
    InternalError,
}
pub type Result<T> = std::result::Result<T, ErrorCode>;
impl From<rusqlite::Error> for ErrorCode {
    fn from(_: rusqlite::Error) -> Self {
        Self::StorageError
    }
}
impl From<serde_json::Error> for ErrorCode {
    fn from(_: serde_json::Error) -> Self {
        Self::InvalidInput
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Running,
    Complete,
    Failed,
    Cancelled,
    TimedOut,
    Unknown,
}
impl Status {
    fn terminal(self) -> bool {
        !matches!(self, Self::Pending | Self::Running)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub target: Target,
    pub revision: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditRecord {
    pub id: u64,
    pub principal: String,
    pub request_id: String,
    pub operation: Operation,
    pub targets: Vec<Target>,
    pub digest: String,
    pub admitted_at: i64,
    pub updated_at: i64,
    pub status: Status,
    pub error: Option<ErrorCode>,
    pub before: Vec<Revision>,
    pub after: Vec<Revision>,
}
/// Contains no bearer token. Only successful trusted authentication constructs it.
#[derive(Clone)]
pub struct Session {
    principal: String,
    credential_digest: String,
}
impl Session {
    pub(crate) fn same_credential(&self, other: &Session) -> bool {
        self.principal == other.principal && self.credential_digest == other.credential_digest
    }
    pub fn principal(&self) -> &str {
        &self.principal
    }
}
/// Non-serializable capability bound to one admitted operation and authenticated credential.
pub struct Permit {
    audit_id: u64,
    signature: String,
    session: Session,
}
pub struct Admission {
    pub record: AuditRecord,
    pub permit: Option<Permit>,
}
#[derive(Debug, Serialize)]
pub struct AuditPage {
    pub records: Vec<AuditRecord>,
    pub next: Option<u64>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostGrants {
    #[serde(default)]
    pub reads: Vec<String>,
    #[serde(default)]
    pub writes: Vec<String>,
    #[serde(default)]
    pub runs: Vec<String>,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|x| format!("{x:02x}")).collect()
}
fn credential_digest(token: &str) -> String {
    hex(digest::digest(&digest::SHA256, token.as_bytes()).as_ref())
}
fn valid_id(id: &str) -> Result<()> {
    identifier(id).map_err(|_| ErrorCode::InvalidInput)
}
fn valid_target(t: &Target) -> Result<()> {
    match t {
        Target::Definition { key } => {
            valid_id(&key.id)?;
            if ![
                "dashboard",
                "ui",
                "vm",
                "function",
                "variable_definition",
                "variable",
                "data_source",
                "monitor_definition",
                "monitor_instance",
            ]
            .contains(&key.kind.as_str())
            {
                return Err(ErrorCode::InvalidInput);
            }
        }
        Target::Resource { id } => valid_id(id)?,
        Target::Client { client_id } => valid_id(client_id)?,
        Target::Assignment { client_id, slot_id } => {
            valid_id(client_id)?;
            if let Some(id) = slot_id {
                valid_id(id)?;
            }
        }
        Target::Live {
            client_id,
            slot_id,
            instance_id,
        } => {
            valid_id(client_id)?;
            valid_id(slot_id)?;
            valid_id(instance_id)?;
        }
    }
    Ok(())
}
fn scope_matches(scope: &Scope, target: &Target) -> bool {
    match (scope, target) {
        (Scope::All, _) => true,
        (
            Scope::Definition {
                definition_kind,
                id,
            },
            Target::Definition { key },
        ) => definition_kind == &key.kind && id.as_ref().is_none_or(|id| id == &key.id),
        (Scope::Resource { id: a }, Target::Resource { id: b }) => a == b,
        (
            Scope::Client {
                client_id,
                slot_id,
                instance_id,
            },
            Target::Client { client_id: c },
        ) => client_id == c && slot_id.is_none() && instance_id.is_none(),
        (
            Scope::Client {
                client_id,
                slot_id,
                instance_id,
            },
            Target::Assignment {
                client_id: c,
                slot_id: s,
            },
        ) => {
            client_id == c
                && instance_id.is_none()
                && slot_id.as_ref().is_none_or(|id| Some(id) == s.as_ref())
        }
        (
            Scope::Client {
                client_id,
                slot_id,
                instance_id,
            },
            Target::Live {
                client_id: c,
                slot_id: s,
                instance_id: i,
            },
        ) => {
            client_id == c
                && slot_id.as_ref().is_none_or(|id| id == s)
                && instance_id.as_ref().is_none_or(|id| id == i)
        }
        _ => false,
    }
}
fn allows(grants: &[Grant], family: Family, action: Action, t: &Target) -> bool {
    grants
        .iter()
        .any(|g| g.family == family && g.actions.contains(&action) && scope_matches(&g.scope, t))
}
fn validate_grants(grants: &[Grant]) -> Result<()> {
    if grants.len() > 128 {
        return Err(ErrorCode::LimitExceeded);
    }
    for g in grants {
        if g.actions.is_empty() {
            return Err(ErrorCode::InvalidInput);
        }
        let valid = match g.family {
            Family::Alerts => &[Action::Read, Action::Acknowledge, Action::Silence, Action::Configure, Action::Observe, Action::Register, Action::Audit][..],
            Family::Authoring => &[
                Action::Read,
                Action::List,
                Action::Validate,
                Action::Save,
                Action::Assign,
                Action::Audit,
            ][..],
            Family::Engine => &[
                Action::Read,
                Action::History,
                Action::Subscribe,
                Action::Write,
                Action::Set,
                Action::Run,
                Action::RunStatus,
                Action::CancelRun,
                Action::ResumeWatch,
                Action::Audit,
            ][..],
            Family::Live => &[
                Action::List,
                Action::Inspect,
                Action::Execute,
                Action::Reload,
                Action::Audit,
            ][..],
        };
        if g.actions.iter().any(|a| !valid.contains(a)) {
            return Err(ErrorCode::InvalidInput);
        }
        match &g.scope {
            Scope::All => (),
            Scope::Definition {
                definition_kind,
                id,
            } => {
                if g.family != Family::Authoring {
                    return Err(ErrorCode::InvalidInput);
                }
                valid_target(&Target::Definition {
                    key: catalog::Key::new(definition_kind, id.as_deref().unwrap_or("any")),
                })?;
            }
            Scope::Resource { id } => {
                if !matches!(g.family, Family::Engine | Family::Alerts) {
                    return Err(ErrorCode::InvalidInput);
                }
                valid_id(id)?;
            }
            Scope::Client {
                client_id,
                slot_id,
                instance_id,
            } => {
                if matches!(g.family, Family::Engine | Family::Alerts)
                    || instance_id.is_some() && g.family == Family::Authoring
                    || instance_id.is_some() && slot_id.is_none()
                {
                    return Err(ErrorCode::InvalidInput);
                }
                valid_id(client_id)?;
                for id in [slot_id, instance_id].into_iter().flatten() {
                    valid_id(id)?;
                }
            }
        }
    }
    Ok(())
}
fn revisions_valid(revisions: &[Revision]) -> Result<()> {
    if revisions.len() > 256 {
        return Err(ErrorCode::LimitExceeded);
    }
    for r in revisions {
        valid_target(&r.target)?;
        valid_id(&r.revision)?;
    }
    Ok(())
}
impl Store {
    /// Trusted HTTP host only, after OIDC session and app-access validation.
    /// The non-hex credential key cannot be authenticated through the Bearer API.
    pub fn browser_alert_session(&mut self, subject:&str)->Result<Session>{
        let principal=format!("oidc-{}",&hex(digest::digest(&digest::SHA256,subject.as_bytes()).as_ref())[..48]);
        let key=format!("browser:{principal}");
        let grants=vec![Grant{family:Family::Alerts,actions:[Action::Read,Action::Acknowledge,Action::Silence].into_iter().collect(),scope:Scope::All}];
        let tx=self.conn.savepoint()?;
        tx.execute("INSERT OR IGNORE INTO agent_principals VALUES(?,1,1,?)",params![principal,serde_json::to_string(&grants)?])?;
        tx.execute("INSERT OR IGNORE INTO agent_credentials VALUES(?,?)",params![key,principal])?;
        tx.commit()?;
        let session=Session{principal,credential_digest:key};self.agent_grants(&session)?;Ok(session)
    }
    /// Operator-only provisioning; never exposed as an authored definition or guest capability.
    pub fn agent_policy_set(
        &mut self,
        id: &str,
        expected: u64,
        enabled: bool,
        grants: &[Grant],
    ) -> Result<u64> {
        valid_id(id)?;
        validate_grants(grants)?;
        let tx = self.conn.savepoint()?;
        let version: Option<u64> = tx
            .query_row(
                "SELECT version FROM agent_principals WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if version.unwrap_or(0) != expected {
            return Err(ErrorCode::Conflict);
        }
        let next = expected.checked_add(1).ok_or(ErrorCode::LimitExceeded)?;
        tx.execute("INSERT INTO agent_principals VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET version=excluded.version,enabled=excluded.enabled,policy=excluded.policy",params![id,next,enabled,serde_json::to_string(grants)?])?;
        tx.commit()?;
        Ok(next)
    }
    /// Generates 256 random bits and returns the credential once. Only its digest is persisted.
    pub fn agent_credential_issue(&mut self, principal: &str) -> Result<String> {
        let enabled: bool = self
            .conn
            .query_row(
                "SELECT enabled FROM agent_principals WHERE id=?",
                [principal],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(ErrorCode::NotFound)?;
        if !enabled {
            return Err(ErrorCode::Forbidden);
        }
        let mut bytes = [0; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| ErrorCode::InternalError)?;
        let token = hex(&bytes);
        self.conn.execute(
            "INSERT INTO agent_credentials VALUES(?,?)",
            params![credential_digest(&token), principal],
        )?;
        Ok(token)
    }
    pub fn agent_credential_revoke(&mut self, token: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM agent_credentials WHERE digest=?",
            [credential_digest(token)],
        )?;
        Ok(())
    }
    pub fn agent_authenticate(&self, token: &str) -> Result<Session> {
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(ErrorCode::Unauthenticated);
        }
        let digest = credential_digest(token);
        let principal:Option<String>=self.conn.query_row("SELECT p.id FROM agent_credentials c JOIN agent_principals p ON p.id=c.principal WHERE c.digest=? AND p.enabled=1",[&digest],|r|r.get(0)).optional()?;
        Ok(Session {
            principal: principal.ok_or(ErrorCode::Unauthenticated)?,
            credential_digest: digest,
        })
    }
    fn agent_grants(&self, session: &Session) -> Result<Vec<Grant>> {
        let policy:Option<String>=self.conn.query_row("SELECT p.policy FROM agent_credentials c JOIN agent_principals p ON p.id=c.principal WHERE c.digest=? AND p.id=? AND p.enabled=1",params![session.credential_digest,session.principal],|r|r.get(0)).optional()?;
        Ok(serde_json::from_str(
            &policy.ok_or(ErrorCode::Unauthenticated)?,
        )?)
    }
    pub fn agent_require(
        &self,
        session: &Session,
        operation: Operation,
        target: &Target,
    ) -> Result<()> {
        valid_target(target)?;
        if !operation.accepts(target) {
            return Err(ErrorCode::InvalidInput);
        }
        let (family, action) = operation.permission();
        if !allows(&self.agent_grants(session)?, family, action, target) {
            return Err(ErrorCode::Forbidden);
        }
        Ok(())
    }
    /// Deployment ceiling; this is trusted operator configuration, not an MCP tool.
    pub fn agent_ceiling_set(&mut self, grants: &[Grant]) -> Result<()> {
        validate_grants(grants)?;
        if grants.iter().any(|g| g.family != Family::Engine) {
            return Err(ErrorCode::InvalidInput);
        }
        self.conn.execute(
            "UPDATE agent_security SET ceiling=? WHERE id=1",
            [serde_json::to_string(grants)?],
        )?;
        Ok(())
    }
    fn agent_ceiling(&self) -> Result<Vec<Grant>> {
        let s: String =
            self.conn
                .query_row("SELECT ceiling FROM agent_security WHERE id=1", [], |r| {
                    r.get(0)
                })?;
        Ok(serde_json::from_str(&s)?)
    }
}
mod audit;
mod authoring;
#[cfg(test)]
mod tests;
mod discovery;
mod assignment;
pub use assignment::AssignmentRequest;

impl Store {
    /// Alert authority uses its own permission family; legacy engine grants confer none.
    pub fn alert_require(&self, session: &Session, action: Action) -> Result<()> {
        if allows(&self.agent_grants(session)?, Family::Alerts, action, &Target::Resource{id:"alerts".into()}) {Ok(())} else {Err(ErrorCode::Forbidden)}
    }
}

#[cfg(test)]
mod browser_session_tests {
 use super::*;
 #[test]
 fn oidc_subjects_have_separate_auditable_alert_identity_without_machine_credentials(){
  let mut store=Store::open(":memory:").unwrap();let a=store.browser_alert_session("issuer#alice").unwrap();let b=store.browser_alert_session("issuer#bob").unwrap();assert_ne!(a.principal,b.principal);
  assert!(store.alert_require(&a,Action::Read).is_ok());assert!(store.alert_require(&a,Action::Configure).is_err());assert!(store.agent_authenticate(&a.credential_digest).is_err());
 }
}
