use std::path::Path;

use serde_json::{Value, json};
use simple_agents_protocol::{
    CommandRequest, ContractError, Counter, Event, Id, MAX_SAFE_INTEGER, SessionState,
    SubmitSession, schemas,
};

const OBSERVE: &[u8] = include_bytes!("../../../contracts/v1/examples/observe.json");
const CODING: &[u8] = include_bytes!("../../../contracts/v1/examples/coding.json");

#[test]
fn observer_and_coding_requests_round_trip_with_distinct_trusted_bindings() {
    let observer = SubmitSession::decode(OBSERVE).unwrap();
    assert!(observer.binding_ids.is_empty());
    assert!(observer.capabilities.is_empty());
    let coding = SubmitSession::decode(CODING).unwrap();
    assert_eq!(coding.binding_ids.len(), 2);
    for request in [observer, coding] {
        assert_eq!(
            SubmitSession::decode(&serde_json::to_vec(&request).unwrap()).unwrap(),
            request
        );
    }
}

#[test]
fn caller_identity_executable_configuration_and_unknown_permissions_are_rejected() {
    for field in [
        "caller_id",
        "executable",
        "credentials",
        "repository",
        "workflow_id",
    ] {
        let mut request: Value = serde_json::from_slice(OBSERVE).unwrap();
        request[field] = json!("injected");
        assert!(
            SubmitSession::decode(&serde_json::to_vec(&request).unwrap()).is_err(),
            "{field}"
        );
    }
    let mut request: Value = serde_json::from_slice(OBSERVE).unwrap();
    request["capabilities"] = json!(["arbitrary_host_command"]);
    assert!(SubmitSession::decode(&serde_json::to_vec(&request).unwrap()).is_err());
    request["capabilities"] = json!([]);
    request["work"]["executable"] = json!("/bin/sh");
    assert!(SubmitSession::decode(&serde_json::to_vec(&request).unwrap()).is_err());
}

#[test]
fn version_counter_text_and_budget_limits_are_explicit() {
    let mut request: Value = serde_json::from_slice(OBSERVE).unwrap();
    request["version"] = json!(2);
    assert!(matches!(
        SubmitSession::decode(&serde_json::to_vec(&request).unwrap()),
        Err(ContractError::UnsupportedVersion)
    ));
    request["version"] = json!(1);
    for number in [0, MAX_SAFE_INTEGER + 1] {
        request["budget"]["retention_secs"] = json!(number);
        assert!(SubmitSession::decode(&serde_json::to_vec(&request).unwrap()).is_err());
    }
    request["budget"]["retention_secs"] = json!(604800);
    request["work"]["instructions"] = json!(" \n ");
    assert!(SubmitSession::decode(&serde_json::to_vec(&request).unwrap()).is_err());
    assert!(Id::new("../escape").is_err());
    assert!(Id::new("valid\ninvalid").is_err());
    assert!(Counter::new(MAX_SAFE_INTEGER).is_ok());
}

#[test]
fn controls_bind_to_current_versions_and_request_ids_without_expanding_authority() {
    for action in [
        json!({"kind":"pause"}),
        json!({"kind":"resume"}),
        json!({"kind":"cancel"}),
        json!({"kind":"steer","instructions":"Inspect the supplied observation"}),
        json!({"kind":"answer","request_id":"question-1","answer":"Use the supplied context"}),
        json!({"kind":"approve","request_id":"permission-1","approved":true}),
    ] {
        let request = json!({"version":1,"idempotency_key":"command-1","expected_resource_version":3,"action":action});
        assert!(CommandRequest::decode(&serde_json::to_vec(&request).unwrap()).is_ok());
    }
    let unbound = json!({"version":1,"idempotency_key":"command-1","action":{"kind":"cancel"}});
    assert!(CommandRequest::decode(&serde_json::to_vec(&unbound).unwrap()).is_err());
    let expanded = json!({"version":1,"idempotency_key":"command-1","expected_resource_version":3,"action":{"kind":"approve","request_id":"permission-1","approved":true,"capability":"arbitrary_host_command"}});
    assert!(CommandRequest::decode(&serde_json::to_vec(&expanded).unwrap()).is_err());
}

#[test]
fn replay_events_have_bounded_safe_sequences_and_terminal_states_are_explicit() {
    let mut event = json!({"version":1,"session_id":"session-1","sequence":1,"attempt":null,"occurred_at_ms":1789117200000u64,"payload":{"kind":"state_changed","from":"queued","to":"running","reason":"admitted","resource_version":2}});
    assert!(Event::decode(&serde_json::to_vec(&event).unwrap()).is_ok());
    event["sequence"] = json!(MAX_SAFE_INTEGER + 1);
    assert!(Event::decode(&serde_json::to_vec(&event).unwrap()).is_err());
    for state in [
        SessionState::Succeeded,
        SessionState::Failed,
        SessionState::Cancelled,
    ] {
        assert!(state.terminal());
    }
    for state in [
        SessionState::Paused,
        SessionState::WaitingForInput,
        SessionState::Interrupted,
        SessionState::Cancelling,
    ] {
        assert!(!state.terminal());
    }
}

#[test]
fn published_schemas_match_the_rust_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/v1");
    for (name, schema) in schemas() {
        let checked: Value = serde_json::from_slice(
            &std::fs::read(root.join(format!("{name}.schema.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            checked,
            serde_json::to_value(schema).unwrap(),
            "schema drift: {name}"
        );
    }
}
