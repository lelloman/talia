# Client registration (P4)

[TALIA-41](https://crumbles.lelloman.com/w/LLPR/TALIA/41) adds durable client and
live-instance registration to the engine, web host and native Android host.
It implements the identity/lifecycle portion of the [MCP contract](mcp-contract.md).
[Saved assignments and package delivery](dashboard-delivery.md) are implemented by
TALIA-42; agent discovery
and live-command delivery follow in the MCP adapter subtasks. Registration itself
provides no engine or agent permissions.

## Identity and local selection

A client has an opaque server-issued ID, platform (`web` or `android`) and editable
display name. Names are labels, not addressing keys. Each slot has an independently
owned ID and one current live-instance ID. Every runtime replacement, including
reload, manual restart and process/page recreation, creates a new live ID. Network
and server reconnects retain a surviving runtime's ID and dirty/edit state.

The web host shares its client credential in origin-local storage. A Web Lock
serializes first registration setup across tabs. Each tab persists its slot,
selection and display configuration in session storage. A lifetime Web Lock detects
cloned session storage: a duplicate tab receives a fresh slot and default local
configuration instead of impersonating its opener. Reload retains the tab's slot
and selection. A newly opened durable slot copies its server-side client default; legacy slots
use local defaults. Changes in one tab do not change another.
Web Locks and secure-context browser APIs are required; localhost development meets
that requirement. Clearing site storage creates a new client on its next load.

Android persists client/slot credentials in private SharedPreferences scoped to the
configured development server port. Dashboard selection and durable-engine mode
survive process recreation. Activity backgrounding retains the runtime and reports
paused; returning to the foreground resumes it. A failed dashboard keeps reporting
its failed state and host foreground status, allowing later inspect/reload support.
Neither client puts credentials or slot ownership tokens into the guest JS context,
compiled package or public diagnostic report.

The selected dashboard and loaded package revision are reported independently of
instance identity. The [delivery layer](dashboard-delivery.md) now supplies revisioned server desired
assignments and per-slot cached baselines. File-based selection remains for legacy
fixtures; durable hosts report the assignment revision they actually loaded.

## Host API and persistence

SQLite schema 7 stores clients, current slots and a tombstone for every used live ID.
Only credential/ownership digests are stored; raw random 256-bit credentials stay
with the host. Repeating registration with the same credential returns the existing
client, including after a lost response. Production enrollment/SSO is still outside
this development service; enrollment is available on its loopback endpoint.

`POST /clients` accepts a strict tagged JSON request and an `Authorization: Bearer`
header carrying the host credential. Replies contain `value` or a predefined `error`,
and the server incarnation. The development Python host forwards the header.

| Operation | Fields |
|---|---|
| `register` | `name`, `platform` |
| `rename` | `name` |
| `status` | none; returns only this client's public registration and slots |
| `connect` | `slot`, `owner`, `live`, optional `previous`, `epoch`, `report` |
| `report` | `slot`, `owner`, `live`, `epoch`, `sequence`, `report` |
| `disconnect` | `slot`, `owner`, `live`, `epoch`, `sequence` |

Report fields are `dashboardId`, `packageRevision`, `lifecycle`, `foreground`,
`dirty`, `editRevision` and `updateAvailable`. Lifecycle is active, paused or failed;
foreground is independent so a failed guest can still have a usable foreground host.
The server adds client/slot/live IDs, connection state, epoch, sequence and last-seen
time. Names, IDs, schemas and counters are bounded; there are at most 256 registered
clients and 64 slots per client. Retired live-ID tombstones are retained to prevent
reuse; archival/deletion policy is future operations work.

Connection epochs increase across reconnects/replacements. Heartbeats increase
sequence within an epoch. Older epochs, out-of-order reports, mismatched ownership
and obsolete live IDs fail. Replacement requires the exact preceding live ID; a
retired ID can never become current again. Within an instance, loaded dashboard/
package identity cannot change, edit revision cannot decrease and dirty cannot clear.
The hosts reconcile the current slot before reconnecting, including after offline
reloads or lost responses; this does not replay guest actions.

Hosts send reports approximately every three seconds. Explicit disconnect marks the
slot unavailable immediately. Abrupt network/process loss becomes unavailable when
its 15-second lease expires; a registry cannot detect an unobserved network failure
instantaneously. Startup marks all saved slots disconnected before accepting traffic.
A new authenticated connection is required to restore availability. Last-known
reports remain inspectable as metadata, never proof of a running reachable VM.

## Live-command boundary

The trusted Rust `client_live_target` gate requires an exact client/slot/live target
and an unexpired connection. It rejects paused/background targets immediately.
Failed guests reject execution, while a connected foreground failed host can pass
the inspect/reload availability check. Old instance IDs always fail, even after the
slot reconnects. There is no deferred command queue.

This gate supplies availability, not agent authorization. The [live MCP adapter](mcp-live.md) also applies [agent permissions](agent-authority.md), revisions and dirty acknowledgement,
and the receiving host must recheck availability/identity immediately before execution.
A heartbeat can race with pause/disconnect, so registry admission alone cannot authorize
later execution. TALIA-45 adds bounded command delivery and isolated guest execution routes.
Unrestricted Rust discovery is for trusted server code; agent listings must be scoped.
The legacy `/engine` transport remains a loopback development interface.

## Verification

- Engine unit tests cover independent slots, exact ownership, retired IDs, report
  ordering, leases, failed/paused targets, and persistent identity after restart.
- `python3 engine/tests/clients.py ENGINE_BINARY` exercises authenticated HTTP,
  strict requests, stale reports, dirty-state fences and real process restart.
- `node dashboard/tests/registration.mjs` exercises duplicate tabs, independent
  selections, reload, dirty reporting, naming and server reconnect in Chromium.
- `python3 dashboard/tests/registration_android.py emulator-5570` exercises native
  lifecycle, failure, reload, process recreation and persisted selection. It rejects
  physical-device serials and removes its test app and reverse mapping afterward.

Build the web and Android clients first. These checks use local fixture servers;
`TALIA_ENGINE_BIN` can point to an out-of-tree engine build. Historical P0–P3 reports
are not relabelled as P4 qualification.
