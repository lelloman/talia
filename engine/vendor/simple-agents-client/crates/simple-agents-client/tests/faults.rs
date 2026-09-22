//! Raw loopback HTTP peers exercise failures that well-formed mock responses
//! cannot express. No live service, external network, or new dependencies.
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::json;
use sha2::{Digest, Sha256};
use simple_agents_client::{Client, Error, Options, protocol::*};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::timeout,
};

const WATCHDOG: Duration = Duration::from_secs(10);

struct Reply {
    bytes: Vec<u8>,
    stall: bool,
}

impl Reply {
    fn close(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            stall: false,
        }
    }

    fn stall(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            stall: true,
        }
    }
}

#[derive(Clone)]
struct Request {
    head: String,
    body: Vec<u8>,
}

struct Peer {
    origin: String,
    requests: Arc<Mutex<Vec<Request>>>,
    task: JoinHandle<()>,
}

impl Peer {
    async fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            let mut replies = VecDeque::from(replies);
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = timeout(WATCHDOG, read_request(&mut stream)).await.unwrap();
                captured.lock().unwrap().push(request);
                // Unexpected retries are also recorded, and cannot hang the test.
                let reply = replies
                    .pop_front()
                    .unwrap_or_else(|| Reply::close(Vec::new()));
                stream.write_all(&reply.bytes).await.unwrap();
                if reply.stall {
                    // Keep the response incomplete until the client closes it.
                    // Unlike a sleep, this synchronizes on actual cancellation.
                    let mut byte = [0];
                    let _ = timeout(WATCHDOG, stream.read(&mut byte)).await;
                }
            }
        });
        Self {
            origin,
            requests,
            task,
        }
    }

    fn client(&self) -> Client {
        Client::new(
            &self.origin,
            "fault-test-secret",
            id("caller"),
            Options {
                timeout: Duration::from_millis(500),
                ..Options::default()
            },
        )
        .unwrap()
    }

    fn requests(&self) -> Vec<Request> {
        assert!(!self.task.is_finished(), "fault peer unexpectedly stopped");
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(stream: &mut TcpStream) -> Request {
    let mut bytes = Vec::new();
    let end = loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() <= 64 * 1024, "request header too large");
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
    };
    let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map_or(0, |(_, length)| length.trim().parse::<usize>().unwrap());
    assert!(length <= 1024 * 1024);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.unwrap();
    Request { head, body }
}

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn fixed(body: &[u8], length: usize) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn chunked(chunks: &[&[u8]], complete: bool) -> Vec<u8> {
    let mut bytes = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    for chunk in chunks {
        bytes.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(b"\r\n");
    }
    if complete {
        bytes.extend_from_slice(b"0\r\n\r\n");
    }
    bytes
}

#[tokio::test]
async fn lost_or_timed_out_mutation_responses_never_trigger_automatic_replay() {
    let submit = SubmitSession::decode(include_bytes!(
        "../../../contracts/v1/examples/observe.json"
    ))
    .unwrap();
    let command = CommandRequest::decode(
        &serde_json::to_vec(&json!({
            "version":1,"idempotency_key":"cancel/original","expected_resource_version":7,
            "action":{"kind":"cancel"}
        }))
        .unwrap(),
    )
    .unwrap();
    for command_call in [false, true] {
        let mut partial_success = fixed(b"{", 100);
        // Exercise body failure after the endpoint's accepted success status,
        // not the separate bounded HTTP-error response path.
        partial_success[9..12].copy_from_slice(if command_call { b"202" } else { b"201" });
        for fault in [
            Reply::close(Vec::new()),
            Reply::stall(Vec::new()),
            Reply::close(partial_success.clone()),
            Reply::stall(partial_success),
        ] {
            let peer = Peer::start(vec![fault]).await;
            let client = peer.client();
            let error = timeout(WATCHDOG, async {
                if command_call {
                    client.command(&id("session"), &command).await.unwrap_err()
                } else {
                    client.submit(&submit).await.unwrap_err()
                }
            })
            .await
            .unwrap();
            assert_eq!(error, Error::Transport);
            assert!(!format!("{error:?} {error}").contains("fault-test-secret"));
            let sent = peer.requests();
            assert_eq!(sent.len(), 1, "uncertain mutation must not be replayed");
            if command_call {
                assert!(
                    sent[0]
                        .head
                        .starts_with("POST /v1/sessions/session/commands ")
                );
                assert_eq!(CommandRequest::decode(&sent[0].body).unwrap(), command);
            } else {
                assert!(sent[0].head.starts_with("POST /v1/sessions "));
                assert_eq!(SubmitSession::decode(&sent[0].body).unwrap(), submit);
            }
        }
    }
}

#[tokio::test]
async fn interrupted_downloads_return_no_partial_artifact_and_explicit_retry_recovers() {
    let body = b"verified artifact";
    let metadata = Artifact {
        artifact_id: id("artifact"),
        name: "evidence".into(),
        media_type: "text/plain".into(),
        bytes: body.len() as u64,
        sha256: hex::encode(Sha256::digest(body)),
    };
    for fault in [
        Reply::close(fixed(&body[..3], body.len())),
        Reply::close(chunked(&[&body[..3]], false)),
        Reply::stall(chunked(&[&body[..3]], false)),
    ] {
        let peer = Peer::start(vec![
            fault,
            Reply::close(chunked(&[&body[..3], &body[3..]], true)),
        ])
        .await;
        let client = peer.client();
        assert_eq!(
            timeout(WATCHDOG, client.artifact(&id("session"), &metadata))
                .await
                .unwrap(),
            Err(Error::Transport)
        );
        assert_eq!(peer.requests().len(), 1);
        assert_eq!(
            timeout(WATCHDOG, client.artifact(&id("session"), &metadata))
                .await
                .unwrap()
                .unwrap(),
            body
        );
        let requests = peer.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].head, requests[1].head);
    }
}

#[tokio::test]
async fn incomplete_event_pages_can_be_refetched_from_the_original_cursor() {
    let body = serde_json::to_vec(&json!({"events":[{"version":1,"session_id":"session",
        "sequence":8,"attempt":null,"occurred_at_ms":1,
        "payload":{"kind":"diagnostic","code":"test","message":"test"}}],
        "next_after":8,"has_more":false}))
    .unwrap();
    // Even complete JSON is not success until the HTTP body has completed.
    for fault in [
        Reply::close(fixed(&body, body.len() + 1)),
        Reply::stall(chunked(&[&body], false)),
    ] {
        let peer = Peer::start(vec![fault, Reply::close(fixed(&body, body.len()))]).await;
        let client = peer.client();
        assert_eq!(
            timeout(WATCHDOG, client.events(&id("session"), 7, 1))
                .await
                .unwrap(),
            Err(Error::Transport)
        );
        assert_eq!(peer.requests().len(), 1);
        let page = timeout(WATCHDOG, client.events(&id("session"), 7, 1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(page.next_after, 8);
        assert_eq!(page.events.len(), 1);
        let requests = peer.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].head, requests[1].head);
        assert!(requests[0].head.contains("after=7&limit=1"));
    }
}

#[tokio::test]
async fn chunked_bodies_enforce_limits_without_content_length_or_end_marker() {
    for artifact in [false, true] {
        let peer = Peer::start(vec![Reply::stall(chunked(&[b"1234", b"56789"], false))]).await;
        let client = Client::new(
            &peer.origin,
            "secret",
            id("caller"),
            Options {
                max_json_bytes: 8,
                max_artifact_bytes: 8,
                timeout: Duration::from_secs(2),
                ..Options::default()
            },
        )
        .unwrap();
        let error = timeout(WATCHDOG, async {
            if artifact {
                client
                    .artifact(
                        &id("session"),
                        &Artifact {
                            artifact_id: id("artifact"),
                            name: "test".into(),
                            media_type: "text/plain".into(),
                            bytes: 8,
                            sha256: "0".repeat(64),
                        },
                    )
                    .await
                    .unwrap_err()
            } else {
                client.events(&id("session"), 0, 1).await.unwrap_err()
            }
        })
        .await
        .unwrap();
        assert_eq!(
            error,
            Error::TooLarge,
            "size bound must reject before waiting for EOF"
        );
        assert_eq!(peer.requests().len(), 1);
    }
}
