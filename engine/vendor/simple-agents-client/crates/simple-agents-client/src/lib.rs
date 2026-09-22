//! Public HTTP client. Mutations are sent once with caller-owned keys. After an
//! uncertain response, reconcile by key or replay the exact original request.
//! Event cursors are advanced by the application only after durable processing.

use std::{fmt, time::Duration};

use protocol::*;
use reqwest::{
    Method, StatusCode, Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
pub use simple_agents_protocol as protocol;

#[derive(Clone, Debug)]
pub struct Options {
    pub timeout: Duration,
    pub connect_timeout: Duration,
    pub max_json_bytes: usize,
    pub max_artifact_bytes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            max_json_bytes: 16 * 1024 * 1024,
            max_artifact_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Errors deliberately exclude response bodies, URLs, work text and credentials.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid client configuration")]
    Configuration,
    #[error("invalid request")]
    InvalidRequest,
    #[error("transport failed; mutation outcome may be uncertain")]
    Transport,
    #[error("authentication rejected")]
    Authentication,
    #[error("HTTP request rejected ({status}, {code:?})")]
    Http {
        status: u16,
        code: Option<ErrorCode>,
        retryable: bool,
    },
    #[error("response exceeds the configured byte bound")]
    TooLarge,
    #[error("response violates the public contract")]
    Contract,
    #[error("artifact integrity check failed")]
    Integrity,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    origin: Url,
    authorization: HeaderValue,
    caller: Id,
    options: Options,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("credentials", &"redacted")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    pub session: Session,
    pub created: bool,
}

/// Hash covers the exact HTTP result bytes, not a reserialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultDocument {
    pub result: SessionResult,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

impl Client {
    /// HTTPS is required except for numeric loopback addresses used by local services.
    /// The origin has no path, query, fragment or user information. Redirects are disabled.
    pub fn new(origin: &str, token: &str, caller: Id, options: Options) -> Result<Self, Error> {
        let origin = Url::parse(origin).map_err(|_| Error::Configuration)?;
        let loopback = origin.host_str().is_some_and(|s| {
            s.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        });
        if !(origin.scheme() == "https" || origin.scheme() == "http" && loopback)
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || token.is_empty()
            || token.len() > 65536
            || !token.bytes().all(|b| b.is_ascii_graphic())
            || options.timeout.is_zero()
            || options.connect_timeout.is_zero()
            || options.max_json_bytes == 0
            || options.max_artifact_bytes == 0
        {
            return Err(Error::Configuration);
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::Configuration)?;
        authorization.set_sensitive(true);
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(options.timeout)
            .connect_timeout(options.connect_timeout)
            .build()
            .map_err(|_| Error::Configuration)?;
        Ok(Self {
            http,
            origin,
            authorization,
            caller,
            options,
        })
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.origin.clone();
        url.path_segments_mut()
            .expect("validated HTTP origin")
            .clear()
            .extend(segments);
        url
    }

    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        statuses: &[StatusCode],
    ) -> Result<reqwest::Response, Error> {
        let mut request = self
            .http
            .request(method, url)
            .header(AUTHORIZATION, self.authorization.clone());
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }
        let response = request.send().await.map_err(|_| Error::Transport)?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(Error::Authentication);
        }
        if !statuses.contains(&status) {
            let bytes = bounded(response, self.options.max_json_bytes.min(65536)).await?;
            let error = serde_json::from_slice::<ErrorResponse>(&bytes).ok();
            return Err(Error::Http {
                status: status.as_u16(),
                code: error.as_ref().map(|e| e.error),
                retryable: error.is_some_and(|e| e.retryable),
            });
        }
        Ok(response)
    }

    async fn json<T: DeserializeOwned>(&self, response: reqwest::Response) -> Result<T, Error> {
        decode(&self.json_bytes(response).await?)
    }

    async fn json_bytes(&self, response: reqwest::Response) -> Result<Vec<u8>, Error> {
        if response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .is_none_or(|h| {
                !h.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("application/json")
            })
        {
            return Err(Error::Contract);
        }
        bounded(response, self.options.max_json_bytes).await
    }

    async fn get<T: DeserializeOwned>(&self, url: Url) -> Result<T, Error> {
        self.json(self.send(Method::GET, url, None, &[StatusCode::OK]).await?)
            .await
    }

    pub async fn submit(&self, request: &SubmitSession) -> Result<Submission, Error> {
        let body = encode(request)?;
        SubmitSession::decode(&body).map_err(|_| Error::InvalidRequest)?;
        let response = self
            .send(
                Method::POST,
                self.url(&["v1", "sessions"]),
                Some(body),
                &[StatusCode::CREATED, StatusCode::OK],
            )
            .await?;
        let created = response.status() == StatusCode::CREATED;
        let session = self.json(response).await?;
        self.check_submission(&session, request)?;
        Ok(Submission { session, created })
    }

    /// Resolve an uncertain submission without creating new work.
    pub async fn by_key(&self, request: &SubmitSession) -> Result<Session, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let session = self
            .get(self.url(&["v1", "sessions", "by-key", request.idempotency_key.as_str()]))
            .await?;
        self.check_submission(&session, request)?;
        Ok(session)
    }

    fn check_submission(&self, session: &Session, request: &SubmitSession) -> Result<(), Error> {
        check_session(session)?;
        if session.caller_id != self.caller
            || session.source != request.source
            || session.profile_id != request.profile_id
            || session.effective.capabilities != request.capabilities
            || session.effective.budget != request.budget
            || session.effective.binding_revisions.len() != request.binding_ids.len()
            || session
                .effective
                .binding_revisions
                .iter()
                .map(|b| &b.binding_id)
                .collect::<std::collections::BTreeSet<_>>()
                != request.binding_ids.iter().collect()
        {
            return Err(Error::Contract);
        }
        Ok(())
    }

    /// Also accepts explicitly shared sessions; ownership is checked on submission only.
    pub async fn session(&self, id: &Id) -> Result<Session, Error> {
        let session: Session = self.get(self.url(&["v1", "sessions", id.as_str()])).await?;
        check_session(&session)?;
        if session.session_id != *id {
            return Err(Error::Contract);
        }
        Ok(session)
    }

    pub async fn sessions(&self, after: Option<&Id>, limit: u32) -> Result<SessionPage, Error> {
        let page: SessionPage = self
            .get(page_url(self.url(&["v1", "sessions"]), after, limit)?)
            .await?;
        check_page(
            page.sessions.iter().map(|s| &s.session_id),
            after,
            page.next_cursor.as_ref(),
            limit,
        )?;
        for session in &page.sessions {
            check_session(session)?;
        }
        Ok(page)
    }

    /// Fetch a bounded page beginning strictly after a durably processed cursor.
    /// Gaps, foreign sessions, unsupported versions and invalid continuation fail.
    pub async fn events(&self, id: &Id, after: u64, limit: u32) -> Result<EventPage, Error> {
        if after > MAX_SAFE_INTEGER {
            return Err(Error::InvalidRequest);
        }
        check_limit(limit)?;
        let mut url = self.url(&["v1", "sessions", id.as_str(), "events"]);
        url.query_pairs_mut()
            .append_pair("after", &after.to_string())
            .append_pair("limit", &limit.to_string());
        let page: EventPage = self.get(url).await?;
        if page.events.len() > limit as usize {
            return Err(Error::Contract);
        }
        let mut cursor = after;
        for event in &page.events {
            Event::decode(&encode(event)?).map_err(|_| Error::Contract)?;
            if event.session_id != *id || event.sequence.get() != cursor + 1 {
                return Err(Error::Contract);
            }
            cursor = event.sequence.get();
        }
        if page.next_after != cursor || page.has_more && page.events.is_empty() {
            return Err(Error::Contract);
        }
        Ok(page)
    }

    pub async fn command(
        &self,
        id: &Id,
        command: &CommandRequest,
    ) -> Result<CommandReceipt, Error> {
        let body = encode(command)?;
        CommandRequest::decode(&body).map_err(|_| Error::InvalidRequest)?;
        let response = self
            .send(
                Method::POST,
                self.url(&["v1", "sessions", id.as_str(), "commands"]),
                Some(body),
                &[StatusCode::ACCEPTED],
            )
            .await?;
        let receipt: CommandReceipt = self.json(response).await?;
        if receipt.session_id != *id {
            return Err(Error::Contract);
        }
        Ok(receipt)
    }

    pub async fn command_receipt(&self, id: &Id, command: &Id) -> Result<CommandReceipt, Error> {
        let receipt: CommandReceipt = self
            .get(self.url(&["v1", "sessions", id.as_str(), "commands", command.as_str()]))
            .await?;
        if receipt.session_id != *id || receipt.command_id != *command {
            return Err(Error::Contract);
        }
        Ok(receipt)
    }

    pub async fn result(&self, id: &Id) -> Result<ResultDocument, Error> {
        let response = self
            .send(
                Method::GET,
                self.url(&["v1", "sessions", id.as_str(), "result"]),
                None,
                &[StatusCode::OK],
            )
            .await?;
        let bytes = self.json_bytes(response).await?;
        Ok(ResultDocument {
            result: decode(&bytes)?,
            sha256: hex::encode(Sha256::digest(&bytes)),
            bytes,
        })
    }

    pub async fn artifacts(
        &self,
        id: &Id,
        after: Option<&Id>,
        limit: u32,
    ) -> Result<ArtifactPage, Error> {
        let url = page_url(
            self.url(&["v1", "sessions", id.as_str(), "artifacts"]),
            after,
            limit,
        )?;
        let page: ArtifactPage = self.get(url).await?;
        check_page(
            page.artifacts.iter().map(|a| &a.artifact_id),
            after,
            page.next_cursor.as_ref(),
            limit,
        )?;
        for artifact in &page.artifacts {
            check_artifact(artifact)?;
        }
        Ok(page)
    }

    /// Caller supplies metadata from this session's artifact listing. Returns bytes
    /// only after checking the advertised length and SHA-256; never writes files.
    pub async fn artifact(&self, id: &Id, metadata: &Artifact) -> Result<Vec<u8>, Error> {
        check_artifact(metadata)?;
        if metadata.bytes > self.options.max_artifact_bytes as u64 {
            return Err(Error::TooLarge);
        }
        let response = self
            .send(
                Method::GET,
                self.url(&[
                    "v1",
                    "sessions",
                    id.as_str(),
                    "artifacts",
                    metadata.artifact_id.as_str(),
                ]),
                None,
                &[StatusCode::OK],
            )
            .await?;
        let bytes = bounded(response, metadata.bytes as usize).await?;
        if bytes.len() as u64 != metadata.bytes
            || hex::encode(Sha256::digest(&bytes)) != metadata.sha256
        {
            return Err(Error::Integrity);
        }
        Ok(bytes)
    }
}

fn check_session(session: &Session) -> Result<(), Error> {
    if session.version != VERSION {
        return Err(Error::Contract);
    }
    Ok(())
}

fn check_artifact(artifact: &Artifact) -> Result<(), Error> {
    if artifact.bytes > MAX_SAFE_INTEGER
        || artifact.sha256.len() != 64
        || !artifact
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Contract);
    }
    Ok(())
}

fn check_limit(limit: u32) -> Result<(), Error> {
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(Error::InvalidRequest);
    }
    Ok(())
}

fn page_url(mut url: Url, after: Option<&Id>, limit: u32) -> Result<Url, Error> {
    check_limit(limit)?;
    url.query_pairs_mut()
        .append_pair("limit", &limit.to_string());
    if let Some(after) = after {
        url.query_pairs_mut().append_pair("after", after.as_str());
    }
    Ok(url)
}

fn check_page<'a>(
    ids: impl Iterator<Item = &'a Id>,
    after: Option<&Id>,
    next: Option<&Id>,
    limit: u32,
) -> Result<(), Error> {
    let mut previous = after;
    let mut count = 0;
    for id in ids {
        if previous.is_some_and(|p| p >= id) {
            return Err(Error::Contract);
        }
        previous = Some(id);
        count += 1;
    }
    if count > limit || next.is_some_and(|n| count == 0 || previous != Some(n)) {
        return Err(Error::Contract);
    }
    Ok(())
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(value).map_err(|_| Error::InvalidRequest)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|_| Error::Contract)
}

async fn bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, Error> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(Error::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Transport)? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(Error::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
