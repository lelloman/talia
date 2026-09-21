# Authored catalog storage (P4)

[TALIA-39](https://crumbles.lelloman.com/w/LLPR/TALIA/39) implements the storage and
compiler foundation of the [MCP contract](mcp-contract.md). It is a trusted Rust
library API. The [MCP authoring adapter](mcp-authoring.md) now exposes authenticated
authoring through that foundation. The
[agent authority layer](agent-authority.md) adds authenticated wrappers and audits;
[client assignments and package delivery](dashboard-delivery.md) connect the saved
packages to both hosts.

## Storage and revisions

SQLite schema 5 adds `authored_definitions`, `dashboard_packages` and a
`catalog_revision` metadata counter. Existing Variables, definitions, monitoring
configuration, private state, samples, history and action records remain in their
P2/P3 tables. Catalog reads project those canonical records rather than maintaining
another copy. Upgrading an existing database preserves its runtime state.

An expected catalog revision protects each save. The counter advances for authoring,
including legacy configuration APIs, through transactional database triggers. Value,
quality, timestamp, history-sample and private-state commits do not advance it.
Revisions are opaque concurrency tokens; a bundle can advance the counter by more
than one. Authored records retain their last changed revision. Projected engine
records conservatively report the current catalog revision. Inspect document/version
fields when determining which engine definition changed.

The public `Store` API is:

- `catalog_snapshot()` returns `catalogRevision` and records (`key`, `revision`,
  `document`) from the owned store.
- `catalog_validate(&ChangeSet)` returns `valid` and structured diagnostics.
- `catalog_save(&ChangeSet)` returns the committed catalog revision, changed keys
  and changed dashboard package revisions, or a structured error.
- `dashboard_package(id)` returns the currently compiled coherent package.

These synchronous calls execute under exclusive engine ownership, never across an
await. Validation executes the same staging/migration/compilation path as save in
a rollback-only savepoint. It leaves no definitions, packages, counters or runtime
state changed and does not reserve the result. Save rechecks revisions and operates
on current runtime state. Existing engine migrations remain bounded and have no I/O.
No authored UI/VM startup or function body is executed by the compiler.

A successful save commits every proposed engine change and compiled package in one
SQLite transaction. Invalid UI, JS, references, schemas, migrations, or package
limits roll back the whole bundle. Nested savepoints allow the agent authority
wrapper to commit catalog activation and its success audit in one transaction.
The unrestricted catalog methods themselves do not create identities or audits.

## Change set and records

`ChangeSet` has `expectedCatalogRevision` and `changes`. Changes are:

```json
{
  "op": "put",
  "key": {"kind": "ui", "id": "notice"},
  "document": {"source": "<Text id=\"Notice\" text=\"Healthy\"/>", "references": []}
}
```

A delete has only `op:"delete"` and `key`. Duplicate keys and deletion of missing
records fail. Unknown input fields are rejected. A new final graph is staged before
reference validation, so bundle order does not affect references. Referenced records
can be deleted only when the same bundle repairs/removes their dependents.

| Kind | Document |
|---|---|
| `dashboard` | `ui` source, `view_model` source, optional `references`, object `params`, and package `grants` |
| `ui` | Fragment `source`, optional shared `references` |
| `vm` | VM definition object-expression `source`, optional shared `references` |
| `function` | Function-expression `source`, optional shared `references` |
| `variable_definition` | Existing P2 `Definition`, including `id`, incremented `version`, source, schemas, dependencies and read policy |
| `variable` | `id`, `definition`, tagged `params`, `history_count`, `history_age_ms`; no mutable runtime fields |
| `data_source` | Existing P3 `DataSource` document |
| `monitor_definition` | Existing P3 Pipeline/Watch definition with its incremented version |
| `monitor_instance` | Existing P3 Pipeline/Watch instance bindings, parameters and scheduling |

Engine document IDs must equal their keys. Source documents use the key as their
name. A shared reference is another `{kind,id}` key. Dashboard references resolve
transitively; UI definitions may reference only other UI definitions. Shared VM
and function dependencies are linked before their dependents. Cycles/missing
references fail even in unused definitions. UI `ScreenRef` targets are checked in
the containing dashboard, where the Screen names are available.

A `variable_definition` put may additionally supply `migration` on the change.
It follows P2 `(state, parameters)` semantics and requires a changed definition.
A new `variable` put may supply `initial:{state,value?}` with tagged values on the
change. Initialization cannot be supplied for an existing Variable. Without it,
state/value are undefined and `has_value` is false. Initial values have unknown
quality and timestamp zero; authoring does not fabricate a measured sample.
Creation-only initial state is an operation input, not a second stored runtime
state. Existing Variable definition identity remains immutable, as in the P2 API;
parameters and retention settings are editable without replacing collected data.

Definition edits advance affected Variable generations/revisions, preserving state
unless migrated. Parameter edits affect only that Variable. Removing/recreating a
Variable uses a fresh generation so its old work cannot publish to the replacement.
P3 activation retains its own migration/generation fences. Monitoring configuration
versions are computed by the adapter; per-definition versions remain explicit.
Unchanged monitoring arrays are compared independent of order, preventing a UI-only
save from reactivating the monitoring configuration.

## Shared compilation

The Rust engine embeds the existing trusted `dashboard/shared/ui.js` compiler in a
bounded QuickJS context. It produces the same version-1 package shape used by web
and native Android; it does not depend on Node or a subprocess compiler in production.
The existing file-based `dashboard/compile.mjs` workflow remains available.

Dashboard UI source compiles directly. Shared UI fragments enter the package's
`definitions` map. Shared VM expressions become `defineVMReference(id, expression)`;
functions become `defineFunction(id, expression)`. Dependency-ordered registrations
precede the dashboard's `defineVM(...)` source. Strict syntax is parsed without
executing authored code; imports/exports and top-level await are unsupported.
Validation cannot prove that authored startup or later actions will succeed.

A package includes its coherent UI, shared definition closure, JS, params and grants.
Only packages whose compiled content changes receive a new `catalog-N-ID` revision.
Updating a shared source updates all affected saved packages; editing one dashboard's
params leaves others unchanged. Already loaded copies remain intact until explicit
reload. Deleting a dashboard removes its saved package, not any already loaded copy.
Historical source/package revisions are not served by this storage API.

Diagnostics identify code/message and, where applicable, key and field path.
UI and QuickJS syntax locations are included when available; UI fragment offsets
account for the compiler's wrapper. Shared JS registrations are generated wrappers,
so locations in those expressions refer to the generated registration source.
Cross-definition graph errors can describe the graph rather than one source line.

## Limits and integration boundary

A change set contains 1–256 unique keys and at most 2 MiB of serialized input.
The final catalog holds at most 2,048 records and 8 MiB of document content, within
existing P2/P3 per-kind limits. Shared sources/linked VM code are bounded to 128 KiB; dashboard UI
source and packages to 256 KiB; declared references to 64 per source/dashboard, with depth 64.
The existing UI node/expansion and QuickJS memory/CPU limits still apply.

Grant syntax is checked here; authenticated resource ceilings are enforced by the
agent authority wrapper. Catalog reads and saves must stay behind trusted server
code. MCP adapters must use the authenticated wrappers for principal-scoped request
deduplication, permissions/audits and scoped discovery.
After a committed engine configuration change, the service integration must notify
computed evaluation/subscribers using the existing engine invalidation/change hooks,
as the legacy transport does. Storage generation checks already fence old work.
The delivery layer serves coherent packages and clients poll for changed revisions;
saving here never reloads a client.

## Verification

Catalog regressions cover conflicts with legacy edits, runtime writes that do not
conflict with authoring, rollback across UI/engine/migrations, validation without
side effects, current-state migration after validation, shared/local changes,
missing/cyclic references, deletion across engine kinds, strict JS/UI diagnostics,
Variable initialization and generation fencing, restart persistence, v4 upgrades,
and package compatibility with the existing shared VM and example dashboard.
Run `cargo test --manifest-path engine/Cargo.toml --offline` (local HTTP fixture
sockets must be permitted). No device is required for this storage/compiler step.
