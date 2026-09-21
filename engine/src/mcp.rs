//! Authenticated authoring dispatch, independent of the MCP transport.
use crate::{
    authority::{ErrorCode, Result, Session, Status},
    catalog::{ChangeSet, Key},
    store::Store,
};
use serde::Deserialize;
use serde_json::{json, Value};
const KINDS: &[&str] = &[
    "dashboard",
    "ui",
    "vm",
    "function",
    "variable_definition",
    "variable",
    "data_source",
    "monitor_definition",
    "monitor_instance",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub name: String,
    #[serde(default = "object")]
    pub arguments: Value,
}
fn object() -> Value {
    json!({})
}
fn limit() -> usize {
    50
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    kind: Option<String>,
    cursor: Option<String>,
    #[serde(default = "limit")]
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    keys: Vec<Key>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    request_id: String,
    change_set: ChangeSet,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Validate {
    change_set: ChangeSet,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusRequest {
    request_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Audit {
    cursor: Option<String>,
    #[serde(default = "limit")]
    limit: usize,
}
fn encode<T: serde::Serialize>(v: T) -> Result<Value> {
    serde_json::to_value(v).map_err(|_| ErrorCode::InternalError)
}
/// Returns a tool result body; failed durable outcomes remain queryable by requestId.
pub fn dispatch(store: &mut Store, session: &Session, request: Request, now: i64) -> Result<Value> {
    let a = request.arguments;
    match request.name.as_str() {
        "dashboard_access_set"=>{store.agent_require_dashboard_admin(session)?;let access:crate::users::Access=serde_json::from_value(a)?;store.dashboard_access_set(session.principal(),&access)},
        "dashboard_access_list"=>{store.agent_require_dashboard_admin(session)?;if a!=json!({}){return Err(ErrorCode::InvalidInput)}store.dashboard_access_list()},
        "definitions_list" => {
            let r: List = serde_json::from_value(a)?;
            if r.kind.as_deref().is_some_and(|k| !KINDS.contains(&k)) {
                return Err(ErrorCode::InvalidInput);
            }
            store.agent_catalog_list(session, r.kind.as_deref(), r.cursor.as_deref(), r.limit)
        }
        "definitions_read" => {
            let r: Read = serde_json::from_value(a)?;
            encode(store.agent_catalog_read(session, &r.keys)?)
        }
        "definitions_validate" => {
            let r: Validate = serde_json::from_value(a)?;
            encode(store.agent_catalog_validate(session, &r.change_set)?)
        }
        "definitions_save" => {
            let r: Save = serde_json::from_value(a)?;
            let saved = store.agent_catalog_save(session, &r.request_id, &r.change_set, now)?;
            let failed = saved.audit.status != Status::Complete;
            let mut value = encode(saved)?;
            if failed {
                value["error"] = value["audit"]["error"].clone();
                if value["error"].is_null() {
                    value["error"] = json!("unknown");
                }
            }
            Ok(value)
        }
        "client_assignment_set" => {
            let r = serde_json::from_value(a)?;
            store.agent_assignment_set(session, &r, now)
        }
        "operation_status" => {
            let r: StatusRequest = serde_json::from_value(a)?;
            Ok(json!({"audit":store.agent_status(session,&r.request_id)?}))
        }
        "audit_list" => {
            let r: Audit = serde_json::from_value(a)?;
            let context = json!(["audit"]);
            let after = r
                .cursor
                .as_deref()
                .map(|c| store.agent_cursor_read(session, &context, c))
                .transpose()?
                .unwrap_or(0);
            let page = store.agent_audit_list(session, after, r.limit)?;
            let next = page
                .next
                .map(|n| store.agent_cursor(session, &context, n))
                .transpose()?;
            Ok(json!({"records":page.records,"nextCursor":next}))
        }
        _ => Err(ErrorCode::InvalidInput),
    }
}
fn obj(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn text_schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":128})
}
/// Advertised schemas are also enforced by serde and canonical catalog validation.
pub fn tools() -> Vec<Value> {
    let key = obj(
        json!({"kind":{"type":"string","enum":KINDS},"id":text_schema()}),
        &["kind", "id"],
    );
    let put = obj(
        json!({"op":{"const":"put"},"key":key,"document":{"type":"object"},"migration":{"type":"string","maxLength":131072},"initial":obj(json!({"state":{},"value":{}}),&["state"])}),
        &["op", "key", "document"],
    );
    let delete = obj(json!({"op":{"const":"delete"},"key":key}), &["op", "key"]);
    let set = obj(
        json!({"expectedCatalogRevision":{"type":"integer","minimum":0},"changes":{"type":"array","minItems":1,"maxItems":256,"items":{"oneOf":[put,delete]}}}),
        &["expectedCatalogRevision", "changes"],
    );
    let page = json!({"cursor":{"type":"string","maxLength":85},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50}});
    let mut list = page.clone();
    list["kind"] = json!({"type":"string","enum":KINDS});
    let mut tools: Vec<Value> = [
        ("definitions_list","Discover authorized keys and revisions, without source. A changed catalog invalidates cursors; restart listing on conflict.",obj(list,&[]),true),
        ("definitions_read","Read up to 32 authorized source/configuration documents at one catalog revision. Reduce keys if the 2 MiB response bound is exceeded.",obj(json!({"keys":{"type":"array","minItems":1,"maxItems":32,"uniqueItems":true,"items":key}}),&["keys"]),true),
        ("definitions_validate","Validate a complete proposed graph without saving. UI is restricted XML source; VM/functions are JS source strings. Returns located compiler diagnostics. See server instructions for document fields.",obj(json!({"changeSet":set}),&["changeSet"]),true),
        ("definitions_save","Atomically create/update/delete an expected-revision bundle. Reuse requestId only for identical retries; query operation_status after an uncertain response. Saving never reloads a live dashboard.",obj(json!({"requestId":text_schema(),"changeSet":set}),&["requestId","changeSet"]),false),
        ("client_assignment_set","Set an existing exact slot or explicit client default (clientDefault:true, omit slotId) with expectedRevision. Requires assignment authority and read access to the dashboard. No live reload; changing a default only affects new slots.",obj(json!({"clientId":text_schema(),"slotId":text_schema(),"clientDefault":{"type":"boolean","default":false},"expectedRevision":{"type":"integer","minimum":1},"assignment":obj(json!({"dashboardId":text_schema(),"params":{"type":"object"},"presentation":obj(json!({"scale":{"type":"number","minimum":0.25,"maximum":8}}),&[])}),&["dashboardId"]),"requestId":text_schema()}),&["clientId","expectedRevision","assignment","requestId"]),false),
        ("operation_status","Read the recorded outcome of this principal's request without replaying effects.",obj(json!({"requestId":text_schema()}),&["requestId"]),true),
        ("audit_list","Page sanitized audit records allowed by current audit grants. No tokens, source bodies, migration state or external responses.",obj(page,&[]),true),
    ].into_iter().map(|(name,description,input,read)|json!({"name":name,"description":description,"inputSchema":input,"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":true,"openWorldHint":false}})).collect();
    tools.push(json!({"name":"dashboard_access_set","description":"Admin/operator sharing policy. Owner must be a Talìa admin OIDC subject. Public means all authenticated users; viewers are exact OIDC subjects. expectedRevision=0 creates private-by-default metadata; changes are versioned and requestId-idempotent. Grants access to all dashboard read resources. Requires global authoring save permission.","inputSchema":obj(json!({"dashboardId":text_schema(),"owner":text_schema(),"public":{"type":"boolean"},"viewers":{"type":"array","items":{"type":"string"},"maxItems":256},"expectedRevision":{"type":"integer","minimum":0},"requestId":text_schema()}),&["dashboardId","owner","public","viewers","expectedRevision","requestId"])}));
    tools.push(json!({"name":"dashboard_access_list","description":"List saved dashboard sharing policies and known user roles for trusted dashboard administration. Requires global authoring save permission.","inputSchema":obj(json!({}),&[])}));
    tools.extend(crate::mcp_engine::tools());
    tools.extend(crate::mcp_live::tools());
    tools.extend(crate::alerts::api::tools());
    tools
}
pub const INSTRUCTIONS:&str="Talìa saved authoring v1. Read catalog sources and revision, edit source/configuration, validate an atomic changeSet, then save with a fresh requestId and expectedCatalogRevision. On conflict reread/reconcile; never blindly retry a changed bundle. Engine tools now read/history/subscribe/poll/unsubscribe, write stored values, invoke computed setters, admit/cancel Pipelines and resume Watches. Live tools now list clients, inspect exact instances, execute bounded async JavaScript bodies through ctx.state/commit/read/write/run, and reload with current edit/assignment revisions and explicit dirty acknowledgement. Live source executes in an isolated invocation context and cannot rewrite Views or install persistent handlers; reusable behavior changes use saved authoring. Paused/disconnected targets fail immediately. Reconcile uncertain live outcomes by requestId without replay. Engine values use tagged envelopes {version:1,value:NODE}; number tags support NaN, Infinity, -Infinity and -0; undefined is [\"undefined\"]. Mutations require stable principal-scoped requestId and return audit/revision outcomes. Subscriptions belong to this MCP connection and must be explicitly re-created after reconnect. Source document fields: ui/vm/function {source:string,references?:[{kind,id}]}; dashboard {ui:string,view_model:string,references?:[{kind,id}],params?:object,grants?:{reads:[],writes:[],runs:[]}}. UI uses Dashboard/Surface/Screen with Row/Column/Scroll containers and Text/Button/etc. Views; shared UI uses <Use definition=\"id\" .../>. JS calls defineVM; shared VM source is a VM object expression; shared function source is a function expression. Config kinds use the canonical engine schemas documented in docs/authoring-storage.md and docs/monitoring.md. Documents never contain resolved secrets. Validate returns safe compiler messages; runtime correctness still requires qualification. Limits: 256 changes, 2 MiB request/read, 128 KiB source, 256 KiB compiled package, 64 reference depth, 2048 records/8 MiB catalog. Credentials and deployment grant ceiling are operator-owned.";
