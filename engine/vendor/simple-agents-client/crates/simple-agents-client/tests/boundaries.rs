use serde_json::json;
use sha2::{Digest, Sha256};
use simple_agents_client::{Client, Error, Options, protocol::*};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

fn id(s: &str) -> Id {
    Id::new(s).unwrap()
}
fn client(origin: &str) -> Client {
    Client::new(origin, "secret-value", id("caller"), Options::default()).unwrap()
}
fn event(sequence: u64) -> serde_json::Value {
    json!({"version":1,"session_id":"session","sequence":sequence,"attempt":null,"occurred_at_ms":1,
        "payload":{"kind":"diagnostic","code":"test","message":"test"}})
}

#[tokio::test]
async fn pagination_errors_and_foreign_or_future_events_fail_before_cursor_advances() {
    let server = MockServer::start().await;
    let client = client(&server.uri());
    for limit in [0, MAX_PAGE_SIZE + 1] {
        assert_eq!(
            client.events(&id("session"), 0, limit).await,
            Err(Error::InvalidRequest)
        );
        assert_eq!(
            client.sessions(None, limit).await,
            Err(Error::InvalidRequest)
        );
        assert_eq!(
            client.artifacts(&id("session"), None, limit).await,
            Err(Error::InvalidRequest)
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    let mut future = event(1);
    future["version"] = json!(2);
    let mut foreign = event(1);
    foreign["session_id"] = json!("foreign");
    for (events, next, more) in [
        (vec![event(2)], 2, false),
        (vec![event(1)], 7, false),
        (vec![future], 1, false),
        (vec![foreign], 1, false),
        (vec![], 0, true),
    ] {
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/v1/sessions/session/events"))
            .and(query_param("limit", "100"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"events":events,"next_after":next,"has_more":more})),
            )
            .mount(&server)
            .await;
        assert_eq!(
            client.events(&id("session"), 0, MAX_PAGE_SIZE).await,
            Err(Error::Contract)
        );
    }
    server.reset().await;
    Mock::given(method("GET"))
        .and(query_param("after", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"events":[event(2)],"next_after":2,"has_more":false})),
        )
        .mount(&server)
        .await;
    assert_eq!(
        client
            .events(&id("session"), 1, 1)
            .await
            .unwrap()
            .next_after,
        2
    );
}

#[tokio::test]
async fn artifacts_validate_query_metadata_download_hash_and_size() {
    let server = MockServer::start().await;
    let bytes = b"verified artifact";
    let metadata = Artifact {
        artifact_id: id("artifact"),
        name: "evidence".into(),
        media_type: "text/plain".into(),
        bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
    };
    Mock::given(path("/v1/sessions/session/artifacts"))
        .and(query_param("limit", "100"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"artifacts":[metadata],"next_cursor":null})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/v1/sessions/session/artifacts/artifact"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .mount(&server)
        .await;
    let client = client(&server.uri());
    assert_eq!(
        client
            .artifacts(&id("session"), None, MAX_PAGE_SIZE)
            .await
            .unwrap()
            .artifacts,
        vec![metadata.clone()]
    );
    assert_eq!(
        client.artifact(&id("session"), &metadata).await.unwrap(),
        bytes
    );
    let mut wrong = metadata.clone();
    wrong.sha256 = "0".repeat(64);
    assert_eq!(
        client.artifact(&id("session"), &wrong).await,
        Err(Error::Integrity)
    );
    wrong.bytes = 1;
    assert_eq!(
        client.artifact(&id("session"), &wrong).await,
        Err(Error::TooLarge)
    );
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"artifacts":[metadata],"next_cursor":"wrong"})),
        )
        .mount(&server)
        .await;
    assert_eq!(
        client.artifacts(&id("session"), None, 1).await,
        Err(Error::Contract)
    );
}

#[tokio::test]
async fn redirects_authentication_oversized_and_malformed_responses_do_not_leak_secrets() {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    let client = Client::new(
        &server.uri(),
        "secret-value",
        id("caller"),
        Options {
            max_json_bytes: 64,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(!format!("{client:?}").contains("secret-value"));
    for (response, expected) in [
        (
            ResponseTemplate::new(302).insert_header("Location", format!("{}/stolen", other.uri())),
            Error::Http {
                status: 302,
                code: None,
                retryable: false,
            },
        ),
        (
            ResponseTemplate::new(401).set_body_string("secret-value"),
            Error::Authentication,
        ),
        (
            ResponseTemplate::new(200).set_body_json(json!({"secret":"secret-value"})),
            Error::Contract,
        ),
        (
            ResponseTemplate::new(200).set_body_raw("a".repeat(65), "application/json"),
            Error::TooLarge,
        ),
        (
            ResponseTemplate::new(200).set_body_string("{}"),
            Error::Contract,
        ),
    ] {
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let error = client.session(&id("session")).await.unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error:?} {error}").contains("secret-value"));
    }
    assert!(other.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn mutations_never_automatically_retry_and_keep_the_original_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error":"unavailable","message":"secret-value","request_id":"request","retryable":true
        })))
        .expect(1)
        .mount(&server)
        .await;
    let request = SubmitSession::decode(include_bytes!(
        "../../../contracts/v1/examples/observe.json"
    ))
    .unwrap();
    let client = client(&server.uri());
    assert_eq!(
        client.submit(&request).await,
        Err(Error::Http {
            status: 503,
            code: Some(ErrorCode::Unavailable),
            retryable: true
        })
    );
    let requests = server.received_requests().await.unwrap();
    let sent: SubmitSession = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(sent, request);
}

#[test]
fn origin_and_token_validation_is_local() {
    for origin in [
        "http://example.com",
        "https://u:p@example.com",
        "https://example.com/path",
        "https://example.com?query",
        "https://example.com#fragment",
    ] {
        assert!(matches!(
            Client::new(origin, "secret", id("caller"), Options::default()),
            Err(Error::Configuration)
        ));
    }
    for token in ["", "a\nb", "a b"] {
        assert!(matches!(
            Client::new(
                "https://example.com",
                token,
                id("caller"),
                Options::default()
            ),
            Err(Error::Configuration)
        ));
    }
}
