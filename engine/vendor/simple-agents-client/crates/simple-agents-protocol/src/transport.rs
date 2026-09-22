//! Version 1 outbound Runner transport; messages carry data, not execution authority.
use crate::{Counter, Id};
use serde::{Deserialize, Serialize};
pub const SUBPROTOCOL: &str = "simple-agents.runner.v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub sequence: Counter,
    pub message_id: Id,
    pub payload: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientFrame {
    ExecutionPermit { assignment_id: Id, nonce: Id },
    Poll,
    Ack { through: u64 },
    Event { message: Message },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerFrame {
    ExecutionPermit {
        assignment_id: Id,
        nonce: Id,
        allowed: bool,
        ttl_ms: u64,
    },
    Welcome {
        generation: Counter,
        command_acked: u64,
        event_acked: u64,
        lease_ms: u64,
    },
    Commands {
        messages: Vec<Message>,
    },
    Ack {
        through: u64,
    },
}
