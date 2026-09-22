//! Talìa adaptation of Simple Agents c3561594ef70b58ba618614b413d205fc03a6f41.
//! Adapted from Crumbles 10c5acf9bc91c83eb76774e5f26a11fb00080285; MIT (c) 2026 lelloman.
//! Standards-based OIDC ID-token validation.
//!
//! Simple Agents redeems browser authorization codes with PKCE and a fixed callback.
//! Discovery, bounded JWKS refresh, issuer/audience/time/signature checks and the
//! regression fixtures are preserved from Crumbles. No provider token is exposed
//! to the supervision UI.

use futures_util::StreamExt;
use jsonwebtoken::{decode, decode_header, errors::ErrorKind, Algorithm, DecodingKey, Validation};
use reqwest::{redirect::Policy, Client, ClientBuilder, Response, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const JWKS_REFRESH_INTERVAL: Duration = Duration::from_secs(3600);
const DISCOVERY_LIMIT: usize = 256 * 1024;
const JWKS_LIMIT: usize = 1024 * 1024;
const TOKEN_RESPONSE_LIMIT: usize = 64 * 1024;
const MAX_JWKS_KEYS: usize = 32;
pub const MAX_ID_TOKEN_BYTES: usize = 16 * 1024;

const CLOCK_SKEW_SECONDS: u64 = 60;
const MAX_ID_TOKEN_AGE_SECONDS: u64 = 10 * 60;

#[derive(Debug, Error)]
pub enum OidcError {
    #[error("OIDC provider request failed: {0}")]
    ProviderRequest(#[from] reqwest::Error),
    #[error("OIDC provider document is invalid: {0}")]
    ProviderDocument(String),
    #[error("OIDC provider response exceeded its size limit")]
    ProviderResponseTooLarge,
    #[error("OIDC authorization code was rejected")]
    AuthorizationCodeRejected,
    #[error("OIDC refresh endpoint unavailable")]
    RefreshUnavailable,
    #[error("OIDC token is invalid: {0}")]
    InvalidToken(&'static str),
    #[error("OIDC signing key is unknown")]
    UnknownKeyId,
}

impl OidcError {
    #[cfg(test)]
    pub fn is_provider_failure(&self) -> bool {
        matches!(
            self,
            Self::ProviderRequest(_) | Self::ProviderDocument(_) | Self::ProviderResponseTooLarge
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn values(&self) -> Vec<&str> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcClaims {
    pub sub: String,
    pub iss: String,
    pub aud: Audience,
    pub exp: u64,
    pub iat: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default)]
    pub azp: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub preferred_username: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OidcDiscovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    introspection_endpoint: Option<String>,
    #[serde(default)]
    response_types_supported: Vec<String>,
    #[serde(default)]
    id_token_signing_alg_values_supported: Vec<String>,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Vec<String>,
}

#[derive(Deserialize)]
struct OidcTokenResponse {
    id_token: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    token_type: Option<String>,
}

// Persisted only inside the authenticated-encrypted server session payload.
#[derive(Clone, Serialize, Deserialize)]
pub struct SessionTokens {
    pub access: String,
    pub refresh: Option<String>,
    pub access_expires: i64,
    pub nonce: String,
    #[serde(default)]
    pub refreshing: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct JwksKey {
    kid: String,
    kty: String,
    #[serde(rename = "use", default)]
    use_sig: Option<String>,
    #[serde(default)]
    alg: Option<String>,
    n: String,
    e: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JwksResponse {
    keys: Vec<JwksKey>,
}

struct JwksCache {
    keys: Vec<JwksKey>,
    fetched_at: Instant,
}

pub struct OidcClient {
    configured_issuer: String,
    client_id: String,
    discovery: OidcDiscovery,
    jwks_cache: Arc<RwLock<JwksCache>>,
    http_client: Client,
    client_secret: Option<String>,
}

impl OidcClient {
    pub async fn new(issuer_url: &str, client_id: &str) -> Result<Self, OidcError> {
        let configured_issuer = normalize_issuer(issuer_url)?;
        if client_id.trim().is_empty() || client_id.len() > 512 {
            return Err(OidcError::ProviderDocument(
                "client ID is empty or too long".to_string(),
            ));
        }
        let http_client = ClientBuilder::new()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(Policy::none())
            .build()?;
        let discovery_url = format!("{configured_issuer}/.well-known/openid-configuration");
        let discovery: OidcDiscovery =
            fetch_json_limited(&http_client, &discovery_url, DISCOVERY_LIMIT).await?;
        validate_discovery(&configured_issuer, &discovery)?;
        let jwks_cache = fetch_jwks(&http_client, &discovery.jwks_uri).await?;

        Ok(Self {
            configured_issuer,
            client_id: client_id.to_string(),
            discovery,
            jwks_cache: Arc::new(RwLock::new(jwks_cache)),
            http_client,
            client_secret: None,
        })
    }

    pub async fn confidential(issuer: &str, id: &str, secret: String) -> Result<Self, OidcError> {
        let mut client = Self::new(issuer, id).await?;
        if secret.len() < 32 || secret.len() > 1024 {
            return Err(OidcError::ProviderDocument("invalid client secret".into()));
        }
        let endpoint = client
            .discovery
            .introspection_endpoint
            .as_ref()
            .ok_or_else(|| OidcError::ProviderDocument("introspection required".into()))?;
        let u = Url::parse(endpoint)
            .map_err(|_| OidcError::ProviderDocument("invalid introspection URL".into()))?;
        validate_provider_url(&u, "introspection")?;
        if u.origin() != Url::parse(&client.configured_issuer).unwrap().origin()
            || !client
                .discovery
                .token_endpoint_auth_methods_supported
                .iter()
                .any(|m| m == "client_secret_basic")
        {
            return Err(OidcError::ProviderDocument(
                "confidential client support required".into(),
            ));
        }
        let origin = Url::parse(&client.configured_issuer).unwrap().origin();
        for endpoint in [
            &client.discovery.authorization_endpoint,
            &client.discovery.token_endpoint,
            &client.discovery.jwks_uri,
        ] {
            if Url::parse(endpoint).unwrap().origin() != origin {
                return Err(OidcError::ProviderDocument(
                    "LelloAuth endpoints must share the configured issuer origin".into(),
                ));
            }
        }
        client.client_secret = Some(secret);
        Ok(client)
    }
    pub async fn active(&self, token: &str, subject: &str) -> Result<bool, OidcError> {
        let response = self
            .http_client
            .post(
                self.discovery
                    .introspection_endpoint
                    .as_ref()
                    .ok_or(OidcError::AuthorizationCodeRejected)?,
            )
            .basic_auth(&self.client_id, self.client_secret.as_ref())
            .form(&[("token", token)])
            .send()
            .await?
            .error_for_status()?;
        let v: serde_json::Value =
            decode_json_response_limited(response, TOKEN_RESPONSE_LIMIT).await?;
        Ok(v["active"] == true
            && v["sub"] == subject
            && v["client_id"] == self.client_id
            && v["iss"] == self.configured_issuer)
    }
    pub fn authorization_url(
        &self,
        redirect: &str,
        state: &str,
        nonce: &str,
        challenge: &str,
    ) -> String {
        let mut url =
            Url::parse(&self.discovery.authorization_endpoint).expect("validated discovery");
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", redirect),
            ("scope", "openid profile email"),
            ("state", state),
            ("nonce", nonce),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
        ]);
        url.into()
    }
    pub async fn validate_id_token(&self, token: &str) -> Result<OidcClaims, OidcError> {
        self.validate_id_token_for_audience(token, &self.client_id)
            .await
    }

    /// Redeem a code bound to the configured callback and PKCE challenge.
    pub async fn exchange_session(
        &self,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
        nonce: &str,
    ) -> Result<(OidcClaims, SessionTokens), OidcError> {
        if code.is_empty() || code.len() > 4_096 {
            return Err(OidcError::AuthorizationCodeRejected);
        }
        if !(43..=128).contains(&code_verifier.len())
            || !code_verifier.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
            })
        {
            return Err(OidcError::AuthorizationCodeRejected);
        }
        if nonce.is_empty() || nonce.len() > 512 {
            return Err(OidcError::InvalidToken("invalid nonce"));
        }

        let mut request = self
            .http_client
            .post(&self.discovery.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("client_id", self.client_id.as_str()),
                ("code_verifier", code_verifier),
            ]);
        if let Some(secret) = &self.client_secret {
            request = request.basic_auth(&self.client_id, Some(secret));
        }
        let response = request.send().await?;
        if response.status().is_client_error() {
            return Err(OidcError::AuthorizationCodeRejected);
        }
        let response = response.error_for_status()?;
        let tokens: OidcTokenResponse =
            decode_json_response_limited(response, TOKEN_RESPONSE_LIMIT).await?;
        let claims = self.validate_id_token(&tokens.id_token).await?;
        if claims.nonce.as_deref() != Some(nonce) {
            return Err(OidcError::InvalidToken("invalid nonce"));
        }
        let session = self.session_tokens(tokens, &claims, nonce)?;
        Ok((claims, session))
    }

    fn session_tokens(
        &self,
        tokens: OidcTokenResponse,
        claims: &OidcClaims,
        nonce: &str,
    ) -> Result<SessionTokens, OidcError> {
        if tokens.access_token.is_empty()
            || tokens.access_token.len() > MAX_ID_TOKEN_BYTES
            || tokens
                .refresh_token
                .as_ref()
                .is_some_and(|t| t.is_empty() || t.len() > MAX_ID_TOKEN_BYTES)
            || tokens
                .token_type
                .as_ref()
                .is_some_and(|t| !t.eq_ignore_ascii_case("Bearer"))
        {
            return Err(OidcError::InvalidToken("invalid token response"));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expires = match tokens.expires_in {
            Some(ttl) if ttl > 0 && ttl <= 86400 * 365 => now + ttl,
            Some(_) => return Err(OidcError::InvalidToken("invalid access lifetime")),
            None => claims.exp,
        };
        if expires <= now {
            return Err(OidcError::InvalidToken("expired access token"));
        }
        Ok(SessionTokens {
            access: tokens.access_token,
            refresh: tokens.refresh_token,
            access_expires: expires as i64,
            nonce: nonce.into(),
            refreshing: false,
        })
    }

    pub async fn refresh_session(
        &self,
        previous: &SessionTokens,
        subject: &str,
    ) -> Result<SessionTokens, OidcError> {
        let refresh = previous
            .refresh
            .as_deref()
            .ok_or(OidcError::AuthorizationCodeRejected)?;
        let response = self
            .http_client
            .post(&self.discovery.token_endpoint)
            .basic_auth(&self.client_id, self.client_secret.as_ref())
            .form(&[("grant_type", "refresh_token"), ("refresh_token", refresh)])
            .send()
            .await?;
        if response.status().is_client_error() {
            return Err(OidcError::AuthorizationCodeRejected);
        }
        if response.status().is_server_error() {
            return Err(OidcError::RefreshUnavailable);
        }
        let tokens: OidcTokenResponse =
            decode_json_response_limited(response.error_for_status()?, TOKEN_RESPONSE_LIMIT)
                .await?;
        // LelloAuth returns a fresh signed ID token on an openid refresh grant.
        let claims = self.validate_id_token(&tokens.id_token).await?;
        if claims.sub != subject || claims.nonce.as_ref().is_some_and(|n| n != &previous.nonce) {
            return Err(OidcError::InvalidToken("refresh identity changed"));
        }
        let mut next = self.session_tokens(tokens, &claims, &previous.nonce)?;
        if next.refresh.is_none() {
            next.refresh = previous.refresh.clone();
        }
        Ok(next)
    }

    async fn validate_id_token_for_audience(
        &self,
        token: &str,
        client_id: &str,
    ) -> Result<OidcClaims, OidcError> {
        if client_id.trim().is_empty() || client_id.len() > 512 {
            return Err(OidcError::InvalidToken("invalid audience"));
        }
        if token.is_empty() || token.len() > MAX_ID_TOKEN_BYTES {
            return Err(OidcError::InvalidToken("invalid length"));
        }
        self.refresh_jwks_if_stale().await?;
        let header = decode_header(token).map_err(|_| OidcError::InvalidToken("malformed"))?;
        if header.alg != Algorithm::RS256 {
            return Err(OidcError::InvalidToken("unsupported signing algorithm"));
        }
        let kid = header
            .kid
            .as_deref()
            .filter(|kid| !kid.is_empty() && kid.len() <= 256)
            .ok_or(OidcError::InvalidToken("missing key ID"))?;

        let decoding_key = match self.find_key(kid) {
            Some(key) => key,
            None => {
                self.refresh_jwks().await?;
                self.find_key(kid).ok_or(OidcError::UnknownKeyId)?
            }
        };
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[client_id]);
        validation.set_issuer(&[self.configured_issuer.as_str()]);
        validation.set_required_spec_claims(&["aud", "exp", "iat", "iss", "sub"]);
        validation.leeway = CLOCK_SKEW_SECONDS;

        let claims = decode::<OidcClaims>(token, &decoding_key, &validation)
            .map_err(|error| OidcError::InvalidToken(jwt_error_reason(error.kind())))?
            .claims;
        validate_claims(&claims, client_id)?;
        Ok(claims)
    }

    async fn refresh_jwks_if_stale(&self) -> Result<(), OidcError> {
        let stale = self
            .jwks_cache
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .fetched_at
            .elapsed()
            >= JWKS_REFRESH_INTERVAL;
        if stale {
            self.refresh_jwks().await?;
        }
        Ok(())
    }

    async fn refresh_jwks(&self) -> Result<(), OidcError> {
        let refreshed = fetch_jwks(&self.http_client, &self.discovery.jwks_uri).await?;
        *self
            .jwks_cache
            .write()
            .unwrap_or_else(|error| error.into_inner()) = refreshed;
        Ok(())
    }

    fn find_key(&self, kid: &str) -> Option<DecodingKey> {
        self.jwks_cache
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .keys
            .iter()
            .find(|key| {
                key.kid == kid
                    && key.kty == "RSA"
                    && key.use_sig.as_deref().is_none_or(|usage| usage == "sig")
                    && key
                        .alg
                        .as_deref()
                        .is_none_or(|algorithm| algorithm == "RS256")
            })
            .and_then(|key| DecodingKey::from_rsa_components(&key.n, &key.e).ok())
    }

    #[cfg(test)]
    pub fn issuer(&self) -> &str {
        &self.configured_issuer
    }
}

fn jwt_error_reason(error: &ErrorKind) -> &'static str {
    match error {
        ErrorKind::InvalidSignature => "invalid signature",
        ErrorKind::MissingRequiredClaim(claim) => match claim.as_str() {
            "aud" => "missing audience",
            "exp" => "missing expiration",
            "iat" => "missing issued-at time",
            "iss" => "missing issuer",
            "nonce" => "missing nonce",
            "sub" => "missing subject",
            _ => "missing required claim",
        },
        ErrorKind::ExpiredSignature => "expired token",
        ErrorKind::InvalidIssuer => "invalid issuer",
        ErrorKind::InvalidAudience => "invalid audience",
        ErrorKind::InvalidSubject => "invalid subject",
        ErrorKind::ImmatureSignature => "token is not active yet",
        ErrorKind::InvalidAlgorithm | ErrorKind::MissingAlgorithm => "invalid algorithm",
        ErrorKind::Json(_) => "invalid claim encoding",
        _ => "claim validation failed",
    }
}

fn normalize_issuer(issuer: &str) -> Result<String, OidcError> {
    let normalized = issuer.trim().trim_end_matches('/');
    if normalized.is_empty() || normalized.len() > 2_048 {
        return Err(OidcError::ProviderDocument(
            "issuer is empty or too long".to_string(),
        ));
    }
    let parsed = Url::parse(normalized)
        .map_err(|_| OidcError::ProviderDocument("issuer is not an absolute URL".to_string()))?;
    validate_provider_url(&parsed, "issuer")?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(OidcError::ProviderDocument(
            "issuer must not contain query or fragment components".to_string(),
        ));
    }
    Ok(normalized.to_string())
}

fn validate_discovery(configured_issuer: &str, discovery: &OidcDiscovery) -> Result<(), OidcError> {
    if discovery.issuer.trim_end_matches('/') != configured_issuer {
        return Err(OidcError::ProviderDocument(
            "discovered issuer does not match configuration".to_string(),
        ));
    }
    for (name, endpoint) in [
        ("authorization endpoint", &discovery.authorization_endpoint),
        ("token endpoint", &discovery.token_endpoint),
        ("JWKS endpoint", &discovery.jwks_uri),
    ] {
        let parsed = Url::parse(endpoint)
            .map_err(|_| OidcError::ProviderDocument(format!("{name} is not an absolute URL")))?;
        validate_provider_url(&parsed, name)?;
    }
    if !discovery
        .response_types_supported
        .iter()
        .any(|response_type| response_type == "code")
    {
        return Err(OidcError::ProviderDocument(
            "authorization-code flow is unsupported".to_string(),
        ));
    }
    if !discovery
        .code_challenge_methods_supported
        .iter()
        .any(|method| method == "S256")
    {
        return Err(OidcError::ProviderDocument(
            "S256 PKCE is unsupported".to_string(),
        ));
    }
    if !discovery
        .token_endpoint_auth_methods_supported
        .iter()
        .any(|method| method == "none")
    {
        return Err(OidcError::ProviderDocument(
            "public-client token exchange is unsupported".to_string(),
        ));
    }
    if !discovery
        .id_token_signing_alg_values_supported
        .iter()
        .any(|algorithm| algorithm == "RS256")
    {
        return Err(OidcError::ProviderDocument(
            "RS256 ID-token signing is unsupported".to_string(),
        ));
    }
    Ok(())
}

fn validate_provider_url(url: &Url, name: &str) -> Result<(), OidcError> {
    let test_loopback = cfg!(test)
        && url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| host == "127.0.0.1" || host == "::1" || host == "localhost");
    if url.scheme() != "https" && !test_loopback {
        return Err(OidcError::ProviderDocument(format!(
            "{name} must use HTTPS"
        )));
    }
    if url.username() != "" || url.password().is_some() || url.host_str().is_none() {
        return Err(OidcError::ProviderDocument(format!(
            "{name} contains invalid authority components"
        )));
    }
    Ok(())
}

async fn fetch_jwks(client: &Client, jwks_uri: &str) -> Result<JwksCache, OidcError> {
    let response: JwksResponse = fetch_json_limited(client, jwks_uri, JWKS_LIMIT).await?;
    if response.keys.is_empty() || response.keys.len() > MAX_JWKS_KEYS {
        return Err(OidcError::ProviderDocument(
            "JWKS key count is outside the supported range".to_string(),
        ));
    }
    Ok(JwksCache {
        keys: response.keys,
        fetched_at: Instant::now(),
    })
}

async fn fetch_json_limited<T: DeserializeOwned>(
    client: &Client,
    url: &str,
    maximum: usize,
) -> Result<T, OidcError> {
    let response = client.get(url).send().await?.error_for_status()?;
    decode_json_response_limited(response, maximum).await
}

async fn decode_json_response_limited<T: DeserializeOwned>(
    response: Response,
    maximum: usize,
) -> Result<T, OidcError> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err(OidcError::ProviderResponseTooLarge);
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > maximum {
            return Err(OidcError::ProviderResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| OidcError::ProviderDocument("response is not valid JSON".to_string()))
}

fn validate_claims(claims: &OidcClaims, client_id: &str) -> Result<(), OidcError> {
    if claims.sub.trim().is_empty() || claims.sub.len() > 512 {
        return Err(OidcError::InvalidToken("invalid subject"));
    }
    if claims
        .nonce
        .as_deref()
        .is_some_and(|nonce| nonce.trim().is_empty() || nonce.len() > 512)
    {
        return Err(OidcError::InvalidToken("invalid nonce"));
    }
    let audiences = claims.aud.values();
    if audiences.is_empty() || audiences.len() > 16 || !audiences.contains(&client_id) {
        return Err(OidcError::InvalidToken("invalid audience"));
    }
    if audiences.len() > 1 && claims.azp.as_deref() != Some(client_id) {
        return Err(OidcError::InvalidToken("invalid authorized party"));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| OidcError::InvalidToken("invalid system time"))?
        .as_secs();
    if claims.iat > now.saturating_add(CLOCK_SKEW_SECONDS)
        || now.saturating_sub(claims.iat) > MAX_ID_TOKEN_AGE_SECONDS + CLOCK_SKEW_SECONDS
    {
        return Err(OidcError::InvalidToken("invalid issued-at time"));
    }
    for value in [
        claims.email.as_deref(),
        claims.name.as_deref(),
        claims.preferred_username.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if value.len() > 1_024 || value.chars().any(char::is_control) {
            return Err(OidcError::InvalidToken("invalid profile claim"));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::{json, Value};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) const CLIENT_ID: &str = "crumbles-browser";

    #[derive(Clone)]
    pub(crate) struct TestKey {
        kid: &'static str,
        private_pem: &'static str,
        n: &'static str,
    }

    impl TestKey {
        pub(crate) fn jwk(&self) -> Value {
            json!({
                "kid": self.kid,
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "n": self.n,
                "e": "AQAB",
            })
        }

        pub(crate) fn sign(&self, claims: &OidcClaims) -> String {
            let mut header = Header::new(Algorithm::RS256);
            header.kid = Some(self.kid.to_string());
            encode(
                &header,
                claims,
                &EncodingKey::from_rsa_pem(self.private_pem.as_bytes()).unwrap(),
            )
            .unwrap()
        }
    }

    pub(crate) fn keys() -> &'static (TestKey, TestKey) {
        static KEYS: (TestKey, TestKey) = (
            TestKey {
                kid: "initial-key",
                private_pem: include_str!("testdata/oidc_initial_private.pem"),
                n: "wwcPnUJEYSLHUHoEf_xsNgMfWagN4kV8vm044hMMqfbMadRt4tLCXdA2n_ST8mA1YXej-5K_030impU0oCNxMUM89t12ZIgE52NtSgT-_6tkxVo_dd1GIY5JICCfNI6TSuVQdRBqcPTTsIm5JRD8orwtU-tHv2TwOrf040rhZABkr5p2OMHKb1G2bFlKUzhEtL-YKAsT1E98v5Hi3f39Y2yrXQaZHu4fFXcYe2wFNnSjxIMLfuqJD65fjwFUal9xjH1omlFJFclJz0cK7qHKFfyJ7MuIPL_Sv4iSbJbf1nTShEDXmq71x1f86c8Y0P8h5kIU70MiICaQG7t8wRN_EQ",
            },
            TestKey {
                kid: "rotated-key",
                private_pem: include_str!("testdata/oidc_rotated_private.pem"),
                n: "qb7Hf8n3ML1fIzFdx-yeuACUOXh2dlptIW_uJDyzXIMz5IVudqD28sFWqD2GWXpBQ_WLu3XXCEhsSV_u6NYdtQioAHevKiKhWSyFPJUE3mRMIwSVaW7fql2XRGxUzcHOt-CvpB5SeXUGW4tKIOjxSLI962_Vn3fe_SBw1GK2BuqXs9JnNRJKCjH6rh2Jdns7ZWbTAPs371vW4Eu50q0jjjo_Pqje8hkqCtss7534DpKa0YMVNFMh_yKEvfb3OroD0SNJyeQIAMHMJKKSCTkn-6LRx-RvphI_DXlO43q5Wvt0nGEneJYBk5KMyfimW51G5cll4Alrn8bSJZjLpXkK8Q",
            },
        );
        &KEYS
    }

    fn discovery(server: &MockServer) -> Value {
        let issuer = server.uri();
        json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": format!("{issuer}/token"),
            "jwks_uri": format!("{issuer}/jwks"),
            "response_types_supported": ["code"],
            "id_token_signing_alg_values_supported": ["RS256"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
        })
    }

    pub(crate) async fn mount_provider(server: &MockServer, jwks: Value) {
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(discovery(server)))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(jwks))
            .mount(server)
            .await;
    }

    pub(crate) fn valid_claims(issuer: &str) -> OidcClaims {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        OidcClaims {
            sub: "provider-subject-123".to_string(),
            iss: issuer.to_string(),
            aud: Audience::One(CLIENT_ID.to_string()),
            exp: now + 300,
            iat: now,
            nonce: Some("browser-validated-nonce".to_string()),
            azp: None,
            email: Some("person@example.com".to_string()),
            email_verified: Some(true),
            name: Some("Provider Person".to_string()),
            preferred_username: Some("provider-person".to_string()),
        }
    }

    #[test]
    fn jwt_failures_are_reduced_to_bounded_non_sensitive_reasons() {
        assert_eq!(
            jwt_error_reason(&ErrorKind::InvalidSignature),
            "invalid signature"
        );
        assert_eq!(
            jwt_error_reason(&ErrorKind::MissingRequiredClaim("nonce".to_string())),
            "missing nonce"
        );
        assert_eq!(
            jwt_error_reason(&ErrorKind::MissingRequiredClaim("private".to_string())),
            "missing required claim"
        );
        assert_eq!(
            jwt_error_reason(&ErrorKind::InvalidAudience),
            "invalid audience"
        );
    }

    #[tokio::test]
    async fn discovery_and_jwks_initialize_one_exact_bounded_provider() {
        let server = MockServer::start().await;
        mount_provider(&server, json!({"keys": [keys().0.jwk()]})).await;

        let client = OidcClient::new(&format!("{}/", server.uri()), CLIENT_ID)
            .await
            .unwrap();

        assert_eq!(client.issuer(), server.uri());
        assert_eq!(
            client
                .jwks_cache
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .keys
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn valid_rs256_id_token_returns_only_validated_claims() {
        let server = MockServer::start().await;
        mount_provider(&server, json!({"keys": [keys().0.jwk()]})).await;
        let client = OidcClient::new(&server.uri(), CLIENT_ID).await.unwrap();
        let token = keys().0.sign(&valid_claims(&server.uri()));

        let claims = client.validate_id_token(&token).await.unwrap();

        assert_eq!(claims.sub, "provider-subject-123");
        assert_eq!(claims.email.as_deref(), Some("person@example.com"));
        assert!(!format!("{:?}", claims).contains(&token));
    }

    #[tokio::test]
    async fn wrong_issuer_audience_expiry_and_old_or_future_iat_fail_closed() {
        let server = MockServer::start().await;
        mount_provider(&server, json!({"keys": [keys().0.jwk()]})).await;
        let client = OidcClient::new(&server.uri(), CLIENT_ID).await.unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut cases = Vec::new();
        let mut wrong_issuer = valid_claims(&server.uri());
        wrong_issuer.iss = "https://wrong-issuer.example".to_string();
        cases.push(wrong_issuer);
        let mut wrong_audience = valid_claims(&server.uri());
        wrong_audience.aud = Audience::One("another-client".to_string());
        cases.push(wrong_audience);
        let mut expired = valid_claims(&server.uri());
        expired.exp = now - CLOCK_SKEW_SECONDS - 1;
        cases.push(expired);
        let mut old = valid_claims(&server.uri());
        old.iat = now - MAX_ID_TOKEN_AGE_SECONDS - CLOCK_SKEW_SECONDS - 1;
        cases.push(old);
        let mut future = valid_claims(&server.uri());
        future.iat = now + CLOCK_SKEW_SECONDS + 1;
        future.exp = future.iat + 300;
        cases.push(future);

        for claims in cases {
            let token = keys().0.sign(&claims);
            assert!(
                matches!(
                    client.validate_id_token(&token).await,
                    Err(OidcError::InvalidToken(_))
                ),
                "{claims:?}"
            );
        }
    }

    #[tokio::test]
    async fn optional_nonce_multi_audience_azp_and_profile_bounds_are_enforced() {
        let server = MockServer::start().await;
        mount_provider(&server, json!({"keys": [keys().0.jwk()]})).await;
        let client = OidcClient::new(&server.uri(), CLIENT_ID).await.unwrap();
        let mut missing_nonce = valid_claims(&server.uri());
        missing_nonce.nonce = None;
        assert!(client
            .validate_id_token(&keys().0.sign(&missing_nonce))
            .await
            .is_ok());
        let mut empty_nonce = valid_claims(&server.uri());
        empty_nonce.nonce = Some(String::new());
        let mut missing_azp = valid_claims(&server.uri());
        missing_azp.aud = Audience::Many(vec![CLIENT_ID.to_string(), "second".to_string()]);
        let mut wrong_azp = missing_azp.clone();
        wrong_azp.azp = Some("second".to_string());
        let mut control_claim = valid_claims(&server.uri());
        control_claim.name = Some("unsafe\nname".to_string());

        for claims in [empty_nonce, missing_azp, wrong_azp, control_claim] {
            assert!(client
                .validate_id_token(&keys().0.sign(&claims))
                .await
                .is_err());
        }

        let mut valid_multi = valid_claims(&server.uri());
        valid_multi.aud = Audience::Many(vec![CLIENT_ID.to_string(), "second".to_string()]);
        valid_multi.azp = Some(CLIENT_ID.to_string());
        assert!(client
            .validate_id_token(&keys().0.sign(&valid_multi))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn malformed_oversized_missing_kid_and_algorithm_confusion_are_rejected() {
        let server = MockServer::start().await;
        mount_provider(&server, json!({"keys": [keys().0.jwk()]})).await;
        let client = OidcClient::new(&server.uri(), CLIENT_ID).await.unwrap();
        assert!(client.validate_id_token("not-a-jwt").await.is_err());
        assert!(client
            .validate_id_token(&"x".repeat(MAX_ID_TOKEN_BYTES + 1))
            .await
            .is_err());

        let claims = valid_claims(&server.uri());
        let missing_kid = encode(
            &Header::new(Algorithm::RS256),
            &claims,
            &EncodingKey::from_rsa_pem(keys().0.private_pem.as_bytes()).unwrap(),
        )
        .unwrap();
        assert!(client.validate_id_token(&missing_kid).await.is_err());
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(keys().0.kid.to_string());
        let confused = encode(&header, &claims, &EncodingKey::from_secret(b"not-rsa")).unwrap();
        assert!(client.validate_id_token(&confused).await.is_err());
    }

    #[tokio::test]
    async fn unknown_kid_forces_one_jwks_refresh_and_accepts_rotated_key() {
        let server = MockServer::start().await;
        mount_provider(&server, json!({"keys": [keys().0.jwk()]})).await;
        let client = OidcClient::new(&server.uri(), CLIENT_ID).await.unwrap();
        server.reset().await;
        mount_provider(&server, json!({"keys": [keys().1.jwk()]})).await;

        let token = keys().1.sign(&valid_claims(&server.uri()));

        assert!(client.validate_id_token(&token).await.is_ok());
        let cache = client
            .jwks_cache
            .read()
            .unwrap_or_else(|error| error.into_inner());
        assert_eq!(cache.keys[0].kid, keys().1.kid);
    }

    #[tokio::test]
    async fn unknown_key_refresh_outage_is_an_observable_provider_failure() {
        let server = MockServer::start().await;
        mount_provider(&server, json!({"keys": [keys().0.jwk()]})).await;
        let client = OidcClient::new(&server.uri(), CLIENT_ID).await.unwrap();
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let error = client
            .validate_id_token(&keys().1.sign(&valid_claims(&server.uri())))
            .await
            .unwrap_err();

        assert!(error.is_provider_failure());
        assert!(!error.to_string().contains("eyJ"));
    }

    #[tokio::test]
    async fn discovery_rejects_mismatch_redirect_insecure_endpoints_and_protocol_gaps() {
        let mutations: [fn(&mut Value); 6] = [
            |document: &mut Value| document["issuer"] = json!("https://wrong.example"),
            |document: &mut Value| document["jwks_uri"] = json!("http://remote.example/jwks"),
            |document: &mut Value| document["response_types_supported"] = json!(["token"]),
            |document: &mut Value| {
                document["id_token_signing_alg_values_supported"] = json!(["HS256"])
            },
            |document: &mut Value| document["code_challenge_methods_supported"] = json!(["plain"]),
            |document: &mut Value| {
                document["token_endpoint_auth_methods_supported"] = json!(["client_secret_basic"])
            },
        ];
        for mutate in mutations {
            let server = MockServer::start().await;
            let mut document = discovery(&server);
            mutate(&mut document);
            Mock::given(method("GET"))
                .and(path("/.well-known/openid-configuration"))
                .respond_with(ResponseTemplate::new(200).set_body_json(document))
                .mount(&server)
                .await;
            assert!(OidcClient::new(&server.uri(), CLIENT_ID).await.is_err());
        }

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
            .mount(&server)
            .await;
        assert!(OidcClient::new(&server.uri(), CLIENT_ID).await.is_err());
    }

    #[tokio::test]
    async fn discovery_and_jwks_response_limits_fail_before_deserialization() {
        let discovery_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(vec![b'x'; DISCOVERY_LIMIT + 1]),
            )
            .mount(&discovery_server)
            .await;
        assert!(matches!(
            OidcClient::new(&discovery_server.uri(), CLIENT_ID).await,
            Err(OidcError::ProviderResponseTooLarge)
        ));

        let jwks_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(discovery(&jwks_server)))
            .mount(&jwks_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; JWKS_LIMIT + 1]))
            .mount(&jwks_server)
            .await;
        assert!(matches!(
            OidcClient::new(&jwks_server.uri(), CLIENT_ID).await,
            Err(OidcError::ProviderResponseTooLarge)
        ));
    }

    #[tokio::test]
    async fn unusable_or_excessive_jwks_keys_fail_closed() {
        for jwks_keys in [json!([]), json!(vec![keys().0.jwk(); MAX_JWKS_KEYS + 1])] {
            let server = MockServer::start().await;
            mount_provider(&server, json!({"keys": jwks_keys})).await;
            assert!(OidcClient::new(&server.uri(), CLIENT_ID).await.is_err());
        }
    }
}
