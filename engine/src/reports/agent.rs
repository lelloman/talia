//! Pinned public Simple Agents client. Exact persisted requests are replayed after ambiguity.
use super::*;
use simple_agents_client::{
    protocol::{Capability, EffectStatus, Id, ResourceBudget, SessionState, SubmitSession},
    Client, Error, Options,
};
use std::{collections::BTreeSet, time::Duration};
use tokio::io::AsyncReadExt;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub origin: String,
    pub caller_id: Id,
    pub token_file: String,
    pub profile_id: Id,
    #[serde(default)]
    pub capabilities: BTreeSet<Capability>,
    #[serde(default)]
    pub binding_ids: BTreeSet<Id>,
    pub budget: ResourceBudget,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Pending {
    pub provider: String,
    pub origin: String,
    pub caller_id: Id,
    pub request: SubmitSession,
    pub session_id: Option<Id>,
    pub state: String,
    pub next_poll: i64,
    pub last_error: Option<String>,
}
pub async fn read_file(path: &str, max: u64) -> Result<Vec<u8>> {
    let f = tokio::fs::File::open(path)
        .await
        .map_err(|_| "report provider file unavailable")?;
    let mut bytes = vec![];
    f.take(max + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "report provider file unavailable")?;
    if bytes.len() as u64 > max {
        return Err("report provider file size limit".into());
    }
    Ok(bytes)
}
pub async fn config(path: &str, name: &str) -> Result<Provider> {
    let config: BTreeMap<String, Provider> = serde_json::from_slice(&read_file(path, 65536).await?)
        .map_err(|_| "invalid report agents configuration")?;
    config
        .get(name)
        .cloned()
        .ok_or("report agent provider missing".into())
}
impl Provider {
    pub async fn client(&self) -> Result<Client> {
        let token = String::from_utf8(read_file(&self.token_file, 16384).await?)
            .map_err(|_| "invalid agent credential")?;
        Client::new(
            &self.origin,
            token.trim(),
            self.caller_id.clone(),
            Options {
                timeout: Duration::from_secs(10),
                connect_timeout: Duration::from_secs(3),
                max_json_bytes: 262144,
                max_artifact_bytes: 262144,
            },
        )
        .map_err(|_| "invalid report agent provider".into())
    }
    pub fn prepare(
        &self,
        name: &str,
        run: &Run,
        step: &Step,
        instructions: &str,
        inputs: &[String],
    ) -> Result<Pending> {
        if self.capabilities.iter().any(|c| {
            matches!(
                c,
                Capability::ExternalMutation | Capability::RepositoryWrite | Capability::HumanInput
            )
        }) {
            return Err(
                "report agents require observational capabilities without human-input requests"
                    .into(),
            );
        }
        let context = json!({"period":{"start":run.period_start,"end":run.created},"inputs":inputs.iter().map(|id|(id.clone(),serde_json::to_value(&run.outputs[id]).unwrap())).collect::<BTreeMap<_,_>>()});
        let request=SubmitSession::decode(json!({"version":1,"idempotency_key":format!("{}-{}",run.id,step.id),"source":{"system":"talia","reference":run.id},"profile_id":self.profile_id,"work":{"instructions":instructions,"context":context.to_string()},"capabilities":self.capabilities,"binding_ids":self.binding_ids,"budget":self.budget}).to_string().as_bytes()).map_err(|_|"invalid report agent request")?;
        Ok(Pending {
            provider: name.into(),
            origin: self.origin.clone(),
            caller_id: self.caller_id.clone(),
            request,
            session_id: None,
            state: "submitting".into(),
            next_poll: 0,
            last_error: None,
        })
    }
}
pub async fn progress(
    provider: &Provider,
    pending: &mut Pending,
    now: i64,
) -> Result<Option<Value>> {
    if provider.origin != pending.origin || provider.caller_id != pending.caller_id {
        return Err("agent provider identity changed; refusing a replacement run".into());
    }
    let client = provider.client().await?;
    pending.next_poll = now + 5000;
    let session = if let Some(id) = &pending.session_id {
        client.by_key(&pending.request).await.and_then(|s| {
            if &s.session_id == id {
                Ok(s)
            } else {
                Err(Error::Contract)
            }
        })
    } else {
        match client.by_key(&pending.request).await {
            Ok(s) => Ok(s),
            Err(Error::Http { status: 404, .. }) => {
                match client.submit(&pending.request).await {
                    Ok(v) => Ok(v.session),
                    // A successful mutation with an unreadable response may already
                    // exist. Reconcile the persisted key before considering a retry.
                    Err(e @ (Error::Contract | Error::TooLarge)) => {
                        pending.last_error = Some(e.to_string());
                        return Ok(None);
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    };
    let session = match session {
        Ok(s) => s,
        Err(e) => return retry(e, pending),
    };
    pending.session_id = Some(session.session_id.clone());
    pending.state = serde_json::to_value(session.state)
        .unwrap()
        .as_str()
        .unwrap()
        .into();
    pending.last_error = None;
    if !session.state.terminal() {
        return Ok(None);
    }
    let document = match client.result(&session.session_id).await {
        Ok(r) => r,
        Err(e) => return retry(e, pending),
    };
    let expected = match session.state {
        SessionState::Succeeded => "succeeded",
        SessionState::Failed => "failed",
        _ => "cancelled",
    };
    if serde_json::to_value(document.result.outcome).unwrap() != expected {
        return Err("agent terminal result does not match session".into());
    }
    if document
        .result
        .effect_receipts
        .iter()
        .any(|r| r.status == EffectStatus::Uncertain)
    {
        return Err("agent result has uncertain external effects".into());
    }
    Ok(Some(
        json!({"session_id":session.session_id,"state":expected,"result":document.result,"sha256":document.sha256}),
    ))
}
fn retry(error: Error, pending: &mut Pending) -> Result<Option<Value>> {
    if matches!(
        error,
        Error::Transport
            | Error::Http {
                status: 409 | 429 | 500..=599,
                ..
            }
    ) {
        pending.last_error = Some(error.to_string());
        Ok(None)
    } else {
        Err(error.to_string())
    }
}
