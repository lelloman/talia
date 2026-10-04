pub mod value;

pub mod definitions;
pub mod monitoring;
pub mod pipelines;
pub mod runtime;
pub mod script;
pub mod sources;
pub mod store;

pub mod scheduling;

pub mod watches;

pub mod catalog;

pub mod authority;

pub mod clients;

pub mod delivery;

pub mod mcp;

pub mod mcp_engine;

pub mod mcp_live;

pub mod alerts;

pub mod users;

pub mod reports;

pub mod telegram;

pub mod ai;
// Transport-agnostic conversation context shared by Telegram and app chat.
pub(crate) mod conversation;
// Server-owned app chat sessions for web and native clients.
pub mod chat;

pub mod native_overview;

pub mod notification_outbox;
