//! Native sessions use the existing server-owned OIDC client. Only a PKCE-bound
//! one-time exchange delivers an opaque Talìa session; provider tokens stay here.
use super::*;
use axum::response::Html;

pub(super) struct Pending {
    challenge: String,
    expires: i64,
    started: bool,
    token: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Start {
    challenge: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Exchange {
    attempt: String,
    verifier: String,
}
#[derive(Deserialize)]
pub(super) struct Authorize {
    attempt: String,
}
fn opaque(s: &str) -> bool {
    s.len() == 43
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub(super) async fn start(State(d): State<Deployment>, Json(q): Json<Start>) -> Response {
    if !opaque(&q.challenge) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut inner = d.inner.lock().unwrap();
    inner.native.retain(|_, p| p.expires > now());
    if inner.native.len() >= 256 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let attempt = random();
    inner.native.insert(
        attempt.clone(),
        Pending {
            challenge: q.challenge,
            expires: now() + 600,
            started: false,
            token: None,
        },
    );
    Json(serde_json::json!({"attempt":attempt,"authorizeUrl":format!("{}/native/authorize?attempt={attempt}",d.origin),"expiresIn":600})).into_response()
}
pub(super) async fn authorize(
    State(d): State<Deployment>,
    headers: HeaderMap,
    Query(q): Query<Authorize>,
) -> Response {
    {
        let mut inner = d.inner.lock().unwrap();
        let Some(p) = inner
            .native
            .get_mut(&q.attempt)
            .filter(|p| p.expires > now() && !p.started)
        else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        p.started = true;
    }
    login_flow(d, headers, Some(q.attempt)).await
}
pub(super) async fn exchange(State(d): State<Deployment>, Json(q): Json<Exchange>) -> Response {
    if !opaque(&q.attempt) || !opaque(&q.verifier) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut inner = d.inner.lock().unwrap();
    let Some(p) = inner.native.get(&q.attempt).filter(|p| p.expires > now()) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if p.challenge != hash(&q.verifier) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if p.token.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    let p = inner.native.remove(&q.attempt).unwrap();
    Json(serde_json::json!({"token":p.token.unwrap()})).into_response()
}
pub(super) fn complete(d: &Deployment, attempt: &str, token: String) -> Response {
    let mut inner = d.inner.lock().unwrap();
    let Some(p) = inner.native.get_mut(attempt).filter(|p| p.expires > now()) else {
        drop(inner);
        let _ = d.remove_session(&hash(&token));
        return StatusCode::UNAUTHORIZED.into_response();
    };
    p.token = Some(token);
    // No credentials in the callback URL. Interception cannot redeem the PKCE grant.
    Html(format!("<!doctype html><meta name=viewport content='width=device-width'><title>Talìa</title><h1>Signed in</h1><p>Return to Talìa to finish connecting.</p><a href='com.lelloman.talia://signin?attempt={attempt}'>Open Talìa</a>")).into_response()
}
pub(super) fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|v| v.strip_prefix("n.").is_some_and(opaque))
}
pub(super) async fn logout(State(d): State<Deployment>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match d.remove_session(&hash(token)) {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(code) => code,
    }
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn pending_expiry_and_repeated_browser_start_fail_closed() {
        let (_server, d) = super::super::tests::fixture().await;
        let attempt = random();
        let verifier = random();
        d.inner.lock().unwrap().native.insert(
            attempt.clone(),
            Pending {
                challenge: hash(&verifier),
                expires: now() + 600,
                started: false,
                token: None,
            },
        );
        assert_eq!(
            exchange(
                State(d.clone()),
                Json(Exchange {
                    attempt: attempt.clone(),
                    verifier: verifier.clone()
                })
            )
            .await
            .status(),
            StatusCode::ACCEPTED
        );
        assert_eq!(
            authorize(
                State(d.clone()),
                HeaderMap::new(),
                Query(Authorize {
                    attempt: attempt.clone()
                })
            )
            .await
            .status(),
            StatusCode::SEE_OTHER
        );
        assert_eq!(
            authorize(
                State(d.clone()),
                HeaderMap::new(),
                Query(Authorize {
                    attempt: attempt.clone()
                })
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        d.inner
            .lock()
            .unwrap()
            .native
            .get_mut(&attempt)
            .unwrap()
            .expires = now() - 1;
        assert_eq!(
            exchange(State(d), Json(Exchange { attempt, verifier }))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}
