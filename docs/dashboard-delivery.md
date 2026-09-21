# Saved dashboard delivery (P4)

[TALIA-42](https://crumbles.lelloman.com/w/LLPR/TALIA/42) connects the saved catalog
to web and native Android. Durable clients fetch coherent server packages through
the authenticated host API. Generated example packages remain only for the legacy
fixture mode; the durable development host rejects `/dashboard/package.json`.

## Desired versus loaded

SQLite schema 8 adds a revisioned default assignment for each initialized client
and independently revisioned slot assignments. An assignment contains `dashboardId`,
object `params` and object `presentation`. Currently `presentation.scale` accepts
0.25–8 CSS pixels per dp for web; Android retains native display density. Parameters
shallowly override the saved dashboard's defaults in both UI and ViewModel input.
Assignment content is limited to 32 KiB and the effective package to 256 KiB.

Opening a new slot copies its client's default. Changing that default later does
not change existing slots. The first initialized default prefers the saved `monitor`
dashboard, otherwise the first saved dashboard ID. Existing slots preserve their
server assignment across reload, page refresh and native process recreation.

Saving an assignment requires its expected revision. `assignment_set` is a trusted
Store API wrapped by the authorized/audited
[`client_assignment_set` MCP tool](mcp-authoring.md). The authenticated host
`select` operation can modify only its own slot, with the slot ownership token.
No authoring sources, guest credentials or engine permissions are exposed by delivery.
Changing desired assignment does not reload any runtime.

Host operations on `POST /clients`, using the existing Bearer credential, are:

| Operation | Fields and result |
|---|---|
| `openSlot` | `slot`, `owner` → initialize if absent and return the desired assignment with `revision` |
| `delivery` | `slot`, `owner` → `{assignment, package}` from one synchronous server read |
| `select` | `slot`, `owner`, `expected`, `assignment` → updated desired assignment |
| `confirmDelivery` | `slot`, `owner`, `revision` → fail if desired assignment changed or dashboard was removed |

A slot can reserve its ownership before a live instance exists, allowing first-load
package preparation. Reservation and registration enforce the same owner. The 64-slot
assignment limit bounds these reservations. A client's default is initialized when
its first slot opens; it is not an implicit assignment for every later report.

Client status includes the desired assignment alongside the report of the loaded
package, loaded `assignmentRevision`, and whether that load used a cached baseline.
Server status derives update availability from these independent revisions. Host UI
checks periodically and shows “Update available”; it never installs during a check.
A failed update check leaves the running dashboard intact. A known deleted/invalid
selection can show an update indication while the old package remains loaded.

## Explicit adoption and cached baselines

Reload prepares one package, validates it, then confirms the assignment revision
before retiring the old runtime. A preparation failure leaves the old runtime alive
and shows a reload error. Confirmation is not a lock: a later save can immediately
make update available again, but cannot change the pinned package being installed.
Startup failure after replacement still stops the new dashboard visibly.

Replacement releases the previous runtime's subscriptions, discards temporary
ViewModel edits, resets edit revision, and creates a fresh live ID. Already committed
server writes and running server operations remain intact. Local explicit selection
saves a desired assignment and follows the same reload path. The web host helper is
`talia.selectDashboard(id, params, presentation)`; Android's explicit launch selection
uses `dashboard` and optional JSON `dashboard_params` extras. Normal process recreation
loads the persisted server selection instead of replaying those selection inputs.

Each host caches the last successfully installed delivery by client ID and slot ID.
The cache includes the coherent package, effective parameters and assignment revision;
another slot using the same dashboard cannot overwrite its baseline. Network/transport
failure permits an explicit reload from that baseline and marks the load as cached.
An explicit server rejection (for example not-found, conflict or forbidden) does not
fall back to stale content. Offline reload does not claim to adopt a newer assignment.
Reconnection resumes reporting; it does not replace the cached runtime automatically.

Android includes its renderer/runtime assets and can reopen its baseline without the
server. Web offline-baseline support here covers an unavailable engine while the
already-open web host and its runtime assets remain available. Offline installation
of the entire web shell/service-worker asset cache is not implemented in this step.

## Development bootstrap and verification

`--seed` imports the monitor/monitoring example **sources** into the canonical catalog
once and compiles them with the server compiler. It does not read generated packages
at runtime or overwrite subsequent saved edits. A durable bootstrap marker prevents
recreating dashboards after the operator deletes them, including deletion of all
packages. This is development fixture provisioning, not an agent authoring endpoint.

Rust tests exercise shared-reference recompilation, isolated slot parameters, default
copying, conflicts, ownership, deletion, persistence and seed non-overwrite. Run:

```sh
cargo test --manifest-path engine/Cargo.toml --offline
cargo build --manifest-path engine/Cargo.toml --offline --bins --examples
node dashboard/tests/delivery.mjs
python3 dashboard/tests/delivery_android.py emulator-5570
```

Set `TALIA_ENGINE_BIN` and `TALIA_DELIVERY_FIXTURE` for a different build directory;
the test scripts default to `/tmp/talia-p3-target/debug`. Build web/Android first.
The fixture editor refuses a database owned by a running engine. Integration tests
stop the fixture engine to edit its saved catalog, then verify surviving clients
show updates and explicitly adopt them. Rust tests separately exercise catalog saves
and package reads without a service restart. These tests do not claim MCP authoring
coverage; [MCP authoring qualification](mcp-authoring.md) now covers that path. Android scripts reject physical devices
and clean up their app and reverse mapping.
