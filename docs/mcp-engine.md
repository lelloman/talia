# MCP engine operations (P4)

[TALIA-44](https://crumbles.lelloman.com/w/LLPR/TALIA/44) exposes the durable server
engine through the same authenticated stdio adapter as [saved authoring](mcp-authoring.md).
These tools operate independently of a loaded dashboard. [Client discovery and live
ViewModel execution/reload](mcp-live.md) are available on both clients.

## Tools and values

Arguments reject unknown fields. IDs are exact. Mutations require a stable
`requestId`; reuse it only with identical arguments.

| Tool | Arguments | Result |
| --- | --- | --- |
| `engine_read` | `id`, optional `timeoutMs` (1–5000, default 5000) | `sample` containing tagged value and public metadata |
| `engine_history` | `id`, optional `limit` (1–100, default 50), `cursor` | Newest-first retained `samples`, `nextCursor` |
| `engine_subscribe` | `ids` (1–8 distinct resources) | `subscriptionId`, initial `values`, `leaseMs` |
| `engine_poll` | `subscriptionId` | Changed `values`, renewed `leaseMs` |
| `engine_unsubscribe` | `subscriptionId` | `released` |
| `engine_write` | `id`, `expectedRevision`, tagged `value`, `requestId` | Durable `audit` for stored Variable write |
| `engine_set` | Same as write | Durable `audit` for computed setter invocation |
| `engine_run` | `id`, `requestId` | `audit` and `admission` with disposition/run ID |
| `engine_run_status` | `runId` | Sanitized durable `run` |
| `engine_cancel_run` | `runId`, `requestId` | Cancellation request `audit` |
| `engine_resume_watch` | `id`, `requestId` | Resume request `audit` |
| `operation_status` | `requestId` | Original `audit`, plus original Pipeline admission when complete |

Values use the existing versioned tagged wire format, including undefined, NaN,
positive/negative infinity, negative zero and nested values. For example:

```json
{"id":"value","expectedRevision":1,"value":{"version":1,"value":["number","Infinity"]},"requestId":"write-value-1"}
```

Samples expose value presence, timestamp, quality, revision, generation and evaluation
status. Variable private state, parameters and source bodies are excluded. Sample
errors are nested under `sample`; a faulted sample is still a successful read.
`monitor.ID` exposes the existing published monitoring state. Run results deliberately
returned by authored code remain visible; exception text is replaced with safe codes.
History pages respect count/age retention and a 1 MiB sample budget. Opaque cursors
bind the credential, principal, current policy, resource and generation; retention
may remove samples between pages.

## Permissions and lifecycle

Use the offline provisioning procedure in [operator setup](mcp-authoring.md).
Engine grants are separate from saved authoring and live permissions. For example:

```json
{"family":"engine","actions":["read","subscribe"],"scope":{"kind":"resource","id":"value"}}
```

History requires its separate history permission. Subscription polling requires current subscribe
permission. Writes, setters, run admission, run status, cancellation and Watch resume
have distinct actions defined in [agent authority](agent-authority.md). Run status
and cancellation resolve the owning Pipeline on the server before checking scope.

Computed reads await the shared server producer; timeout or reader cancellation
stops that wait only. A producer can finish for other consumers or its cache. Setters
retain the existing five-second deadline and revision/generation guards. Their trusted
host guard rechecks the agent's primary setter permission before continuations,
commits and effects, plus read/write permission for nested resources. Nested reads
also recheck after awaiting their result. Await yields execution; no SQLite
transaction or global execution lock spans I/O.

Subscriptions belong to the authenticated principal, credential and adapter
connection. Poll within their 60-second idle lease. Samples coalesce; polling is not
an event log and does not repeatedly evaluate getters. Unsubscribe, graceful stdio
closure, expiry and permission revocation release their producer demand without
removing another client's subscriptions. A one-second sweep performs idle cleanup.
Bounds are 128 connections, 16 subscriptions per connection, 128 subscriptions total,
8 resources per subscription and 16 active calls per connection. Existing adapter
frame/body, ingress and HTTP timeout limits also apply.

The adapter creates private connection/call identities, forwards SDK cancellation
and closes its backend session on graceful shutdown. These controls are not agent
tools. Cancellation skips unstarted work and fences later setter effects. Effects
and explicit private-state commits already completed survive. Abrupt adapter death
or response loss is not proof of cancellation: reconcile by request ID.

## Durable outcomes

Mutation admission is audited before effects. Exact retries return the recorded
outcome without replay; changed arguments conflict. Revision conflicts report the
current revision when authorized. Pipeline run admission and its success audit share
one SQLite transaction. Admission means queued/accepted, not completed; inspect its
run ID for completion. Admitted Pipelines have server-owned lifetimes, independent
of the MCP wait. Explicit cancellation preserves committed effects and may leave an
in-flight external effect unknown. Watch resume clears its fault without resetting
its internal state.

If recording completion fails after an effect, the request can remain unresolved;
restart marks running work unknown and never replays it. A completed cancellation
audit means the request was handled, not that earlier effects were undone. Audit
queries expose safe outcomes/digests, not original values or script exceptions.

## Verification

```sh
cargo test --manifest-path engine/Cargo.toml --locked --offline
cargo build --manifest-path engine/Cargo.toml --locked --offline --bins --examples
python3 engine/tests/mcp_engine.py
bash dashboard/build-web.sh
bash dashboard/build-android.sh
python3 engine/tests/emulator.py   # separate terminal: disposable emulator-5570
python3 engine/tests/mcp_engine.py --clients
adb -s emulator-5570 emu kill
```

The external stdio test covers exceptional values, revisions, permissions, bounded
reads, subscription ownership/cleanup, actual SDK cancellation, Pipeline deduplication,
Watch resume, reconnect and crash reconciliation. Rust tests also inject audit failures
and use virtual time for lease expiry. Client checks consume an MCP-written infinite
value through existing subscriptions on Chromium and native Android, render it in
the chart, and confirm the authored ViewModel remains clean. Android checks reject
physical-device serials. Production deployment and live ViewModel control are outside
this ticket.
