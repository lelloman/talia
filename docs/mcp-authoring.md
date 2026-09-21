# MCP authoring adapter (P4)

[TALIA-43](https://crumbles.lelloman.com/w/LLPR/TALIA/43) exposes saved authoring to
external MCP clients. `talia-mcp` is a stdio adapter using the
[official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk), pinned
in Cargo.lock. It forwards authenticated operations to the running Rust engine.
The engine remains the sole SQLite owner; dashboard/monitoring edits activate
without restarting it. Both web and native Android consume the same compiled
packages. No client-side authoring implementation or embedded chat is introduced.

This implements the authoring portion of the [MCP contract](mcp-contract.md).
[Engine-operation tools](mcp-engine.md) are also available. [Client discovery and live
inspection/execution/reload](mcp-live.md) are available on both clients. Production network exposure and SSO remain later work.

## Operator setup

Build all binaries with `cargo build --manifest-path engine/Cargo.toml --locked --offline --bins`.
The following policy is an example for a trusted development author. Narrow its
scopes before assigning a more restricted role. Store the JSON outside the repository:

```json
{
  "principal": "dashboard-author",
  "expectedVersion": 0,
  "enabled": true,
  "grants": [{
    "family": "authoring",
    "actions": ["list", "read", "validate", "save", "assign", "audit"],
    "scope": {"kind": "all"}
  }],
  "ceiling": [{
    "family": "engine",
    "actions": ["read", "write"],
    "scope": {"kind": "resource", "id": "value"}
  }]
}
```

Provision before starting the engine, using the same database path as the service:

```sh
engine/target/debug/talia-agent /path/to/engine.db /path/to/policy.json /path/to/author.token
engine/target/debug/talia-engine /path/to/engine.db 18745 --seed
```

`talia-agent` refuses to run while the service owns the database lock. Policy,
optional deployment ceiling and credential issuance are one database transaction.
The credential output must be a new file; it is created with mode 0600. The token
never appears in command arguments or console output; only its digest is persisted
in SQLite. A failed provisioning attempt can leave an empty output file. A failed
credential-file write revokes the issued credential and reports that the policy
was already saved. The optional ceiling changes the deployment-wide dashboard
grant ceiling, not the agent's direct engine permissions. Omitting it preserves
the current ceiling; a new database defaults to an empty ceiling.

To update/disable a principal, stop the service, increment the policy's expected
version to the last returned version, and use `--policy-only` instead of an output
file. `enabled:false` disables all that principal's credentials. Policy-only edits
do not mint new credentials. This is development operator provisioning, not a
runtime public provisioning API; ordinary authored definitions need no downtime.

Configure the external MCP client to launch this process (use absolute paths):

```json
{
  "command": "/path/to/talia/engine/target/debug/talia-mcp",
  "args": ["http://127.0.0.1:18745", "/path/to/author.token"]
}
```

The adapter reads its credential file at launch. It permits only an IPv4 loopback
HTTP origin, disables proxies and redirects, and never takes a principal from tool
arguments. The engine authenticates and checks current policy on every operation.
Stdout carries only MCP messages; shutdown follows stdin closure. SDK negotiation
is qualified with protocol versions 2025-11-25 and 2026-07-28.

`POST /agent` is a private backend bridge, **not an HTTP MCP transport**. It rejects
Origin-bearing requests and requires an agent Bearer credential. The dashboard
proxy does not expose it. The existing `/engine` development API remains trusted,
unauthenticated loopback infrastructure; this change does not secure that API for
remote deployment. Host registration/delivery still uses its separate `/clients`
credentials. Keep these development services local.

## Tools and results

`tools/list` publishes input schemas; initialization supplies authoring guidance.
All application argument objects reject unknown fields. Tools return the same
JSON in `structuredContent` and a text content block. Application failures set
`isError:true` and use stable error codes. Unknown tool names are protocol errors.

| Tool | Arguments | Result |
|---|---|---|
| `definitions_list` | Optional `kind`, `cursor`, `limit` | `catalogRevision`, authorized key/revision `records`, `nextCursor` |
| `definitions_read` | 1–32 unique `keys` | Coherent `catalogRevision` and source/configuration `records` |
| `definitions_validate` | `changeSet` | `valid`, located `diagnostics`; no saved effects |
| `definitions_save` | `changeSet`, `requestId` | `audit`, new-save `receipt`, safe `diagnostics`; failed outcomes also have `error` |
| `client_assignment_set` | `clientId`, `slotId` **or** `clientDefault:true`, `expectedRevision`, `assignment`, `requestId` | `audit`, new-save `assignment`; no live reload |
| `operation_status` | `requestId` | `audit`: this principal's durable outcome, without replay |
| `audit_list` | Optional `cursor`, `limit` | Scope-filtered sanitized `records`, `nextCursor` |

Audit record fields retain the existing authority layer's snake_case names; tool
arguments, catalog snapshots and assignments use the documented camelCase names.
A save receipt contains `catalogRevision`, changed keys and authorized affected
package revisions. Exact mutation retries return the original audit record, with
no new receipt/assignment and no repeated compilation, migration or effect. Query
current definitions/assignments separately; an old success is not a current snapshot.
The current audit tool filters by policy scope; caller-selected target filters are
not yet exposed. Client discovery comes with live control; assignment callers must
currently obtain exact client/slot IDs and revisions through trusted host status.

List permission grants metadata discovery only; read permission is separate.
Every requested read key must be authorized and present. Listings default to 50,
allow 1–100, and use HMAC-authenticated cursors bound to the principal, credential,
current policy and filters. Definition cursors also bind the catalog revision.
A stale cursor returns `conflict`; restart listing rather than guessing an offset.
Audit cursors advance across hidden rows without returning their contents. An empty
page can still have a next cursor. Audit listings require explicit audit grants.

## Editing source and configuration

Read the catalog revision and the definitions to edit. Use the canonical document
formats in [authoring storage](authoring-storage.md),
[UI/VM contract](../dashboard/contracts/v1/README.md),
[Variable definitions](engine-definitions.md) and [monitoring](monitoring.md).
UI remains restricted XML-style source; VM/function code remains JavaScript strings.
The adapter does not introduce another UI or engine configuration language.

For example, after reading revision 12, call `definitions_validate` with:

```json
{
  "changeSet": {
    "expectedCatalogRevision": 12,
    "changes": [{
      "op": "put",
      "key": {"kind": "ui", "id": "notice"},
      "document": {"source": "<Text id=\"Notice\" text=\"Healthy\"/>"}
    }]
  }
}
```

Then call `definitions_save` with that same `changeSet` and a fresh `requestId`.
Use `op:"delete"` and a key for deletion. Related changes belong in one bundle;
removing a referenced record requires repairing/removing its dependents in that
bundle. Validation reserves nothing; save repeats checks against current revisions.
A conflict requires rereading and reconciling, followed by a new request ID.

Compiler/parser errors retain bounded source messages, key, path and available
line/column when the definition is readable. Other errors return safe repair hints.
Migration exceptions never expose private state, even to definition readers.
Non-readable diagnostic locations are stripped. The durable audit stores only
sanitized error codes/digests, never source bodies or exception text. These checks
cannot prove that an authored dashboard will behave correctly at runtime.

Definition activation and its success audit commit atomically. Computed-variable
subscribers are refreshed after activation; durable generation/revision fencing
prevents stale work from committing. Scheduled monitoring observes the new config
without service restart. Saved package changes become updates available to clients;
loaded runtimes keep their existing coherent package until explicit reload.

Assignment changes require exact assignment scope and read access to the selected
dashboard. The target default/slot must already be initialized. An assignment uses
`{dashboardId, params?, presentation?}`. Defaults affect only subsequently opened
slots. The assignment and success audit share a transaction; failure changes neither.

## Bounds, cancellation and recovery

The catalog's source, graph, migration and package limits still apply. Additionally:

- MCP input frames are capped at 2,359,296 bytes before JSON parsing; oversized frames
  terminate that transport. Forwarded tool bodies are capped at 2 MiB.
- Reads return at most 2 MiB; reduce the key count if needed. Backend response reads
  are capped at 2,359,296 bytes; reduce audit page size on a limit error.
- Each adapter allows 16 concurrent calls without a waiting queue. The existing engine
  ingress/active-work bounds remain 128. HTTP waits time out after 30 seconds.

Cancellation before forwarding skips work. Once a mutation reaches the engine,
losing/cancelling the wait does not establish whether it committed, and never undoes
effects. Query `operation_status` using the original request ID. Accepted synchronous
authoring finishes its transaction; no SQLite transaction spans I/O. Restart recovery
uses the existing pending/running audit rules and never automatically replays work.
Async engine setters additionally fence later effects on cancellation; see
[engine operation lifecycle](mcp-engine.md).
Malformed/unroutable requests are rejected before admission; valid mutation requests
record permission denials, conflicts, validation failures and committed outcomes.

## Verification

```sh
cargo test --manifest-path engine/Cargo.toml --locked --offline
cargo build --manifest-path engine/Cargo.toml --locked --offline --bins --examples
python3 engine/tests/mcp_authoring.py
bash dashboard/build-web.sh
bash dashboard/build-android.sh
python3 engine/tests/emulator.py                 # separate terminal; disposable emulator-5570
python3 engine/tests/mcp_authoring.py --clients
adb -s emulator-5570 emu kill
```

The external Python MCP client launches the adapter over stdio, authors shared
interactive UI/VM/functions plus automatic Prometheus/Pipeline/Watch configuration,
and checks conflicts, rollback, permissions, assignments, diagnostics, audits,
computed subscription refresh, deletion, restart and revocation. `--clients` loads
that authored package on Chromium and native Android and clicks its shared action.
The Android test rejects physical-device serials and cleans up its app/port mapping.
Unit tests additionally cover cursor scope binding, failed audit rollback, atomic
operator setup and input-frame limits. Existing delivery tests cover explicit
adoption and cached/offline behavior; P0 qualification remains historical evidence.
