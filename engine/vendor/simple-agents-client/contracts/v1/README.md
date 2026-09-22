# Simple Agents session contract v1

Owner: [LLPR/AGENT-4](https://crumbles.lelloman.com/w/LLPR/AGENT/4).

This contract describes the standalone service's public session operations.
The [Rust HTTP client](../../docs/CLIENT.md) consumes these operations using the
same protocol types and is tested against the service over HTTP.

## Transport and versions

The public API uses authenticated JSON over HTTPS under `/v1`. Its JSON Schema
2020-12 documents are generated from `simple-agents-protocol`, committed beside
this file, and checked for drift in `cargo test`. The Rust crate is usable
without the service or a Crumbles checkout.

Requests, sessions and events carry `version: 1`. Unknown request fields,
unknown enum variants and unsupported versions fail explicitly. A breaking
field/enum/semantic change requires a new major version and endpoint namespace;
servers must not silently reinterpret old requests. A deployment advertises
supported versions through its published artifact documentation. Clients must
select a supported version before submission; old clients fail closed on a new
event variant rather than acknowledge evidence they did not understand.

IDs are 1–128 ASCII characters, start alphanumeric, and otherwise use only
letters, digits, `.`, `_`, `:`, `/`, and `-`. Encode IDs as single URI path
segments. Sequence numbers, resource versions, positive budgets and timestamps
must fit the exact JSON integer range 1 through 9007199254740991. Timestamps
are UTC Unix milliseconds. A stream cursor of 0 means the beginning.

Submission bodies are limited to 5 MiB, instructions to 1 MiB and context to
4 MiB; all limits apply to UTF-8 bytes. Control bodies are limited to 64 KiB and
answers/steering to 16 KiB. Required work text cannot be blank; text cannot
contain NUL. JSON Schema string lengths are an additional character bound; the
Rust decoders enforce byte limits. Event envelopes are at most 128 KiB; large
evidence is represented by authorized artifacts, preserving its full bytes.

## Operations and response schemas

| Method and path | Request / success response | Semantics |
| --- | --- | --- |
| `POST /v1/sessions` | `submit-session` / `session` | 201 for a durable new session; 200 for an identical replay. |
| `GET /v1/sessions/by-key/{key}` | — / `session` | Resolve an uncertain submission by authenticated caller and idempotency key. |
| `GET /v1/sessions?after=...&limit=...` | — / `session-page` | Live caller-authorized listing; exclusive session ID cursor. |
| `GET /v1/sessions/{session}` | — / `session` | Current authorized state and frozen effective settings. |
| `POST /v1/sessions/{session}/commands` | `command` / `command-receipt` | 202 once durably queued; a receipt is not proof that a worker applied the command. |
| `GET /v1/sessions/{session}/commands/{command}` | — / `command-receipt` | Reconcile pending/applied/rejected control delivery. |
| `GET /v1/sessions/{session}/events?after=0&limit=...` | — / `event-page` | Durable ordered evidence; `after` is exclusive. |
| `GET /v1/sessions/{session}/events/stream?after=0` | — / SSE `event` records | Replay then follow; SSE `id` equals the per-session sequence. |
| `GET /v1/sessions/{session}/artifacts?after=...&limit=...` | — / `artifact-page` | Authorized metadata listing; exclusive artifact ID cursor. |
| `GET /v1/sessions/{session}/artifacts/{artifact}` | — / exact bytes | Downloaded as application/octet-stream; byte count and SHA-256 match metadata. |
| `GET /v1/sessions/{session}/result` | — / `result` | 200 only after terminal settlement; 409 while work/reconciliation is pending. |

All list limits are 1–100, default 50, defined by the protocol crate's
`MAX_PAGE_SIZE` and `DEFAULT_PAGE_SIZE`. Session and artifact pages return the
last ID as `next_cursor` when more results exist; pass it as `after` on the next
request. Lists reflect live state and authorization on every read, so newly
inserted IDs before the cursor require a fresh listing. They do not promise a
stable snapshot. An event gap is never silently skipped. A client
resumes from its last durably stored sequence and deduplicates earlier events.
Conflicting content at a known sequence is an integrity failure, not a replay.

Every schema basename in the table refers to `<name>.schema.json` in this
directory. Health endpoints are outside this contract and expose no session
data. There is no unauthenticated session, artifact or transcript endpoint.

## Identity, capabilities and idempotency

The credential authenticates the caller; submission cannot supply `caller_id`.
`source` is an opaque correlation reference and never grants authority. Neither
a Crumbles ticket nor a Git repository is required. Examples:
[observer](examples/observe.json), [coding](examples/coding.json).

The service resolves `profile_id` and `binding_ids` against trusted, authorized
configuration. Bindings identify engine/tool/repository/publication permissions;
requests cannot supply executable paths, raw credentials, forge authority or
arbitrary host mounts. A binding identifier alone conveys no access. Work text
and subsequent human text are untrusted input, never configuration overrides.

Requested capabilities and resource budgets must fit the caller, profile,
binding and host ceilings. Reject requests exceeding those ceilings instead of
silently broadening authority or shortening requested retention. Freeze the
effective profile/engine/binding revisions and budgets with the accepted session.
Later configuration changes do not rewrite a running session's binding; current
revocation and permission checks can still stop its authority. A restart or
resume requires revalidation and cannot revive a revoked capability.

Submission uniqueness is `(authenticated caller, idempotency_key)`. Compare
the complete normalized typed request, including work, source, capabilities,
bindings and budgets. Equivalent object ordering is not a conflict; changed
content is `idempotency_conflict`. Concurrent identical submissions produce one
session and one initial event transaction. Replays return that original frozen
binding after current access checks, not a new binding from changed profiles.

Keep a minimal idempotency tombstone after evidence expiry for the caller/key
namespace lifetime; an old key must never start new work simply because its
transcript expired. Return `evidence_expired` when its retained result is gone.
Caller identity recreation must not reuse an old identity namespace.

## State, human interaction and controls

New work is `queued`; admission creates a durable attempt before `running`.
The normal completion states are `succeeded`, `failed`, and `cancelled`.
`waiting_for_input`, `pausing`, `paused`, `cancelling`, and `interrupted` are
nonterminal. State changes increment resource version and append an event
atomically. Attempt counters are monotonic and never reused after a restart.

Commands carry an idempotency key and expected resource version. Uniqueness is
`(caller, session, command key)`; identical replay returns the existing receipt
even after the session has advanced. A changed command conflicts. A new command
with stale state fails without changing the session. Authorization is checked
at acceptance and application; queued commands do not survive authority
revocation as executable permissions.

Pause and cancel first record intent (`pausing`/`cancelling`), then fence the
exact worker before reporting a settled state. Resume is allowed only for
retained paused/interrupted work with valid authority and reconciled cleanup;
it creates a new attempt using a bound checkpoint. Terminal sessions cannot
be resumed. Human replies may resume only the pending request's exact attempt
binding, according to the engine's supported checkpoint/reinvocation mechanism.

Questions and permission requests have immutable request IDs, attempt bindings
and expiry times. An approval references the existing request and cannot name
new permissions. It can approve only that request's capability within the
session's frozen ceiling and current authorization. Repeated, expired or
superseded answers/approvals have explicit outcomes. Steering cannot alter the
engine, credentials, budgets or permission ceiling.

## Evidence, recovery and errors

Persist initial instructions/context, model output, tool calls/results, stdout,
stderr, human interactions, state changes, artifacts and results. Stream only
durable events. Preserve complete evidence for successful/all-good outcomes as
well as failures. Credentials must remain outside those evidence paths.

Nonterminal evidence is retained. At terminal settlement, freeze
`retain_until_ms = settlement time + effective retention_secs`; reject arithmetic
overflow. Storage pressure must not silently delete evidence before that time.
Evidence budget exhaustion must stop/refuse additional execution explicitly and
preserve already promised evidence and a durable failure outcome; reserve space
for that control record. Workspace cleanup cannot remove the only retained copy.

Every external mutation has an operation ID and binding-specific receipt.
`uncertain` means reconcile authoritative external state before retrying that
operation or starting replacement work. Durable command deduplication does not
provide exactly-once external execution. A worker/process restart never adopts
an unverified survivor or reruns the model because publication acknowledgment
was lost. Client completion alone has no ticket workflow semantics.

Errors use `error.schema.json`, with a non-secret message, request ID and
`retryable` indicator:

| HTTP | Codes |
| --- | --- |
| 400 | `invalid_request`, `unsupported_version` |
| 401 | `unauthorized` |
| 403 | `forbidden` |
| 404 | `not_found` (also hides other callers' objects) |
| 409 | `idempotency_conflict`, `stale_version`, `invalid_state`, `cursor_expired` |
| 410 | `request_expired`, `evidence_expired` |
| 413 | `invalid_request` for an oversized body |
| 429 | `capacity_unavailable` |
| 503 | `unavailable` |
| 500 | `internal` |

After timeouts or retryable failures, reconcile using the original submission
or command key. Never manufacture a new key as an automatic retry. Read and
control access remain current even when a Runner is offline; the standalone
authorization model must not depend on a Crumbles login or database read.

## Existing supervision compatibility

`simple_agents_protocol::legacy_supervision` preserves the pinned Crumbles v1
wire implementation and its three original contract tests, with the fixture at
`contracts/legacy/runner-supervision-v1.json`. This is a separate compatibility
namespace, not an alias for this public API. Legacy Crumbles user IDs and ticket
targets are converted only by the authenticated client integration described
in AGENT-21; they cannot mint a generic caller identity or grant.

Regenerate schemas after an intentional contract change:

```sh
cargo run --locked -p simple-agents-protocol --example export-schema -- contracts/v1
./scripts/check
```
