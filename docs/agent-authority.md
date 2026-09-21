# Agent authority and audits (P4)

[TALIA-40](https://crumbles.lelloman.com/w/LLPR/TALIA/40) implements the trusted Rust
authority layer in `engine/src/authority.rs`. It supports the
[MCP contract](mcp-contract.md). The [authoring adapter](mcp-authoring.md) now uses
this boundary for MCP tools and the authenticated private `/agent` bridge.
[Engine tools](mcp-engine.md) enforce resource permissions, connection-owned leases
and guarded asynchronous setter effects through the same boundary.
The legacy `/engine` loopback development transport remains unauthenticated.
Upcoming engine and live-control adapters must also use this boundary.

## Credentials and permissions

Trusted operator code creates or updates versioned principal policies with
`agent_policy_set`, issues credentials with `agent_credential_issue`, and revokes
them with `agent_credential_revoke`. Credentials contain 256 random bits, are returned
once, and are stored only as SHA-256 digests. `agent_authenticate` produces an opaque
Session. Tool arguments, authored definitions and guest JS cannot construct one.
Provisioning is not an agent tool or an authored catalog kind.

Policies grant explicit actions in three independent families: `authoring`, `engine`
and `live`. There is no implicit permission inheritance. A representative grant is:

```json
{"family":"engine","actions":["read","subscribe"],"scope":{"kind":"resource","id":"disk_free"}}
```

Scopes match all resources in their family, an exact resource, a definition kind
and optional exact ID, or an exact client with optional slot and instance IDs.
Instance scope requires a slot. IDs are exact matches, not prefixes or patterns.
Policies reject unsupported fields, family/action combinations and invalid scopes;
each policy has at most 128 grants. Empty policies deny everything.

Read, write, setter, run, run-status, cancellation and Watch-resume rights are
separate. Authoring read, list, validate, save and assignment rights are separate
from engine operations. Live list, inspect, execute and reload rights are separate
from both. Audit access is an explicit action within each family. Current policy
and credential validity are checked again before dispatch and after asynchronous
continuations, so revocation applies to existing Sessions.

Live script engine access requires both the agent's current engine permission and
the loaded dashboard package's host grant. `agent_live_effect` also requires an
active live-execution Permit. Host grants must come from trusted loaded-package
state, never injected code or caller arguments. The future runtime adapter must
carry this opaque authority through asynchronous work and retire it on completion,
cancellation, timeout, reload or disconnect. It must not place it in guest state.

## Durable admission and outcomes

`agent_admit` records a request before returning an opaque dispatch Permit. Its
identity is `(principal, request_id)`. Repeating the same request returns its prior
record without another Permit. Changed arguments under that identity produce an
audited conflict and leave the original outcome intact. There are at most 128
pending/running requests; excess requests fail immediately. There is no work queue.

Adapters must derive targets and revisions from trusted resolved resources, validate
operation-specific inputs, and use this sequence:

1. Admit the request and persist its audit. Without a Permit, dispatch nothing.
2. Call `agent_start` immediately before first dispatch, without an intervening await.
3. Call `agent_check_permit` before later effects and after awaits; live engine
   operations also require `agent_live_effect`.
4. Call `agent_finish` with a predefined outcome/error and affected revisions.

An adapter must resolve an action/run ID to its actual owning resource before
authorizing it. Caller-supplied permission targets are not authoritative. Likewise,
client instance freshness, dirty acknowledgements and connection/lifecycle checks
belong to the upcoming routing adapters, alongside these permission checks.

Completion is terminal and cannot grant another dispatch. A known outcome can be
recorded after credential revocation, but revocation prohibits new effects. Effects
already performed are never undone. A failure to persist admission prevents dispatch;
failure to record an external effect's result leaves an uncertain outcome, not a
reason to retry. Startup calls `agent_recover` before accepting traffic: pending work
fails as cancelled, running work becomes unknown, and neither is replayed.

Audit records contain principal, request ID, operation, exact targets, before/after
revision metadata, timestamps, status and a predefined error code. Raw arguments,
source code, result bodies, tokens and arbitrary exception messages are excluded.
A keyed HMAC-SHA-256 digest detects changed arguments without exposing low-entropy
inputs through an unkeyed hash. Its random key stays in the server database.
Malformed/unauthenticated requests rejected before admission have no audit record;
well-formed authenticated permission denials and request conflicts are audited.

`agent_status` is principal-scoped and rechecks the request's permission.
`agent_audit_list` requires audit rights for every target of a returned record;
pages contain at most 100 records and scan at most 1,000 rows. Cursors can reveal
gaps, but never hidden record contents. Audit retention/archiving is not implemented
here. Protect and back up the SQLite database as server-private data.

## Authored catalog integration

Agent-facing adapters use `agent_catalog_list`, `agent_catalog_read`,
`agent_catalog_validate` and `agent_catalog_save`, rather than unrestricted Store
APIs. Metadata listings require list scope; source reads require read scope.
The older `agent_catalog_snapshot` helper filters records by read scope. Saves require save permission for every changed
key and read permission for explicitly referenced sources. Validation requires
validate permission and sanitizes diagnostics that could contain migration state.

The operator's `agent_ceiling_set` policy bounds resource grants in changed compiled
dashboard packages. Its default is empty. Setting this ceiling does not grant an
agent engine access. Authoring permission still intentionally permits changes to
server behavior through engine definitions; allocate that authority accordingly.

Save admission is durable first. Catalog activation and the successful audit record
then commit in one transaction. A failed audit write rolls back the catalog change.
Audit failures use predefined codes. Agent responses also return safe located
compiler diagnostics and repair hints; migration exception text remains private.
Package revision receipts are filtered by read permission. Retries return the original audit outcome without
rerunning migrations or compilation. Validation remains rollback-only.

SQLite schema 6 adds private policy, credential, security-key, request-deduplication
and audit tables. Upgrades preserve existing catalog and engine state. These tables
are not authored definitions, client state or dashboard package content.

## Verification

Run `cargo test --manifest-path engine/Cargo.toml --offline`, allowing local fixture
sockets. Authority regressions cover independent families/scopes, revocation,
live-engine permission intersection, bounded admission, deduplication, scoped
discovery/audits, secret redaction, transactional audit failure, restart recovery
and schema upgrades. This backend step requires no Android device.
