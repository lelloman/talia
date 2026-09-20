# MCP authoring and live control contract (P4)

This is the implementation contract for [TALIA-38](https://crumbles.lelloman.com/w/LLPR/TALIA/38),
following the approved P4 refinement. It specifies behavior to implement; it does
not claim that the MCP tools or client registry already exist. Crumbles owns the
execution plan and status. P1–P3 qualification remains historical evidence.

External MCP agents author Talìa. Embedded chat, production deployment and actual
Crumbles/Simple Agents task execution are outside P4. Web and native Android use
the same contracts; Android qualification uses an emulator only.

## Existing boundaries

The [dashboard contract](../dashboard/contracts/v1/README.md) supplies the UI
language, shared references, VM isolation and explicit revision adoption.
[Engine transport](engine-transport.md), [values](engine-values.md),
[computed execution](engine-computed.md) and [monitoring](monitoring.md) supply
engine operations and recovery semantics. P4 adds authenticated agent access,
authored storage, client registration and live command routing around these.

No arbitrary guest I/O, View rewriting, engine rollback or automatic replay is
introduced. JavaScript still yields during I/O; short state commits retain the
existing revision guards. Server collection continues without connected clients.
The development live-script hooks and caller-supplied transport client IDs are
not credentials and must not become an authorization bypass.

## Identity and client selection

| Identity | Lifetime and authority |
|---|---|
| Agent principal | Established by trusted MCP connection authentication; never accepted from a tool argument claiming an agent name |
| Client ID | Persistent opaque registration ID; one Android installation or browser profile/origin, with an editable display name |
| Slot ID | One independently configured dashboard surface: one browser tab or the Android app's dashboard surface |
| Live-instance ID | One loaded runtime in a slot; replacement/reload gets a new ID, never reused |
| Server incarnation / connection epoch | Existing transport recovery identities; separate from client, slot and live-instance IDs |

Names such as “Kitchen tablet” help discovery but are not unique addressing keys.
Commands use exact IDs. A client registration does not itself grant engine or MCP
permissions. A browser's tabs share its client ID, but their slot IDs and selected
dashboards are independent. A duplicated tab must acquire a distinct slot ID.
Refresh can retain the tab's slot selection; it always replaces the live instance.
A newly opened tab starts a new slot using the client default. Android retains its
selection across process recreation. Clearing application/browser storage creates
a new registration; old records remain identifiable as disconnected.

A slot has a revisioned desired assignment: dashboard ID, parameter overrides and
presentation settings. Browser tab selection is not stored as one shared mutable
browser-wide dashboard choice. A client default initializes new slots; changing it
does not silently reassign existing slots. Changing an existing slot's desired
assignment requires its expected assignment revision and takes effect on explicit
reload. The registry reports both desired assignment and currently loaded package.
A local explicit select-and-load operation follows the same revision/adoption rules.

Client reports include client/slot/live IDs, platform, loaded dashboard and package
revision, desired assignment revision, last-seen time, connection status, lifecycle,
dirty flag, edit revision and update availability. Lifecycle is active, paused or
failed; dirty is independent. Disconnected reports are last-known observations,
not evidence of a currently inspectable VM. Registry records survive server restart;
clients are disconnected until they reconnect and prove registration ownership.
A surviving runtime can keep its live ID across a network/server reconnect.
Host credentials, not guest JavaScript, own registration and status reporting.

## Authored catalog and atomic activation

Agents edit restricted UI source and JavaScript directly. Saved records have an
opaque revision, stable key `{kind,id}`, source/configuration and explicit references
and parameters. Kinds cover dashboards, shared UI, shared VM, shared functions,
Variable definitions/instances, DataSources and Pipeline/Watch definitions/instances.
Existing P2/P3 configuration fields, tagged values and migration rules remain
canonical; catalog adapters must not create a second conflicting configuration model.
UI and JavaScript source are strings; structured configuration stays structured.

A change set contains `expectedCatalogRevision` and `changes`. Each change is
`{op:"put",key,document}` or `{op:"delete",key}`; repeated keys are invalid.
The catalog revision covers the complete authoring graph. Any saved change,
including changes through older configuration APIs, advances it. Ordinary value,
Watch-state or Pipeline-result updates do not. A global expected revision is a
conservative conflict boundary: unrelated concurrent authoring can also conflict.
Agents reread and reconcile; the server never silently merges or overwrites.

Validation resolves the proposed final graph, validates UI/source syntax, references,
cycles, grants, parameter/schema compatibility, package limits and required bounded
migrations. It has no external effects and does not execute application startup or
arbitrary user I/O. Diagnostics include code, message, definition key, field path,
and line/column where available. Syntax validity cannot guarantee runtime success.
Deleting a referenced definition fails unless the same bundle removes or repairs
all references. Credential references are allowed; resolved secret values are not.

Save repeats validation against current revisions. All definitions, affected engine
configuration/state migrations, compiled package revisions and the successful audit
record commit together, or none do. No SQLite transaction spans I/O. Concurrent
engine state changes during preparation must be revalidated or fail conflict;
a migration cannot overwrite newly committed state. Return the new catalog revision,
changed definition revisions and affected dashboard package revisions. Validation
results are not a reservation and cannot bypass save-time checks.

Shared definitions remain references. Changing one invalidates every affected saved
package; changing an instance's parameters only changes that instance. Packages
contain one coherent closure of referenced definitions. Engine definitions activate
at the successful commit, without restart and with P2/P3 generation fencing.
Loaded dashboards retain their old package and show update available. Saving never
executes a live edit or reloads a dashboard. Removed saved dashboards cannot be
selected anew; already loaded instances retain their package until explicit action.

## Capability boundaries and audit

P4 uses three independently grantable permission families. A grant carries allowed
operations and resource/target scope; possession of one family implies neither of
the others. Read/discovery access is scoped too, not globally available by default.

| Family | Operations |
|---|---|
| Authoring | Discover/read/validate/save definitions and configure desired assignments |
| Engine | Read/history/subscribe, write/set, Pipeline admission and allowed control actions |
| Live control | Discover client status, inspect a VM, execute temporary code, reload an exact live instance |

An agent authoring code is deliberately configuring future application behavior;
authoring scope therefore includes control over capability declarations. A saved
grant cannot exceed the deployment's allowed resource ceiling. Granting authoring
is not equivalent to making that agent incapable of indirect future effects.
Live-injected engine calls require both the loaded dashboard's grants and the
invoking agent's engine authorization, including asynchronous continuations. Live
control alone cannot use a dashboard as a route to otherwise forbidden actions.
Ordinary authored user interactions retain their configured host grants.

Enforce authorization at the server and host dispatch boundaries, including after
waits and before new effects. Scripts cannot forge principal, target, request IDs
or grants. Production credential provisioning/SSO is outside this contract, but P4
requires a trusted authenticated principal; loopback location alone is insufficient.

Audit each MCP mutation/action and its rejected or interrupted outcome: audit ID,
principal, operation, target IDs, request ID, time, before/after revisions when
applicable, outcome and sanitized error code. Live edits and reloads are included.
Do not log bearer tokens, resolved credentials, raw VM state, arbitrary code bodies
or unfiltered external responses. Use source digests and authorized definition
revision references for attribution. Audit reads require appropriate scope.

Persist admission before dispatching effects. If admission/audit persistence fails,
do not start the action. If outcome recording fails after an effect, preserve the
admission as unresolved/unknown; never claim rollback or replay automatically.
No distributed transaction with a client or external endpoint is implied.

## Tool surface and results

The names and logical arguments below define Talìa's v1 application interface.
They are independent of the MCP transport/SDK chosen by implementation. Each tool
publishes a bounded input schema and rejects unknown fields. Opaque pagination
cursors and finite page limits apply to listings. Sources are returned as text;
engine/VM values use the [lossless envelope](engine-values.md), not lossy JSON.

| Tool | Logical arguments and result |
|---|---|
| `definitions_list` | `kind?`, `cursor?`, `limit?` → authorized keys, revisions, catalog revision, next cursor |
| `definitions_read` | `keys` → coherent documents and catalog revision |
| `definitions_validate` | change set → validity and located diagnostics; no saved effects |
| `definitions_save` | change set, `requestId` → atomically committed revisions or conflict/validation failure |
| `clients_list` | `clientId?`, `cursor?`, `limit?` → registrations, slots and timestamped last-known live status |
| `client_assignment_set` | `clientId`, `slotId` (or explicit client default), `expectedRevision`, `assignment`, `requestId` → new desired-assignment revision; no reload |
| `engine_read` / `engine_history` | resource ID and existing operation options → tagged samples/history |
| `engine_subscribe` | resource IDs → principal-owned subscription ID and current snapshot |
| `engine_poll` / `engine_unsubscribe` | subscription ID → bounded snapshot updates / released lease |
| `engine_write` / `engine_set` | resource ID, `expectedRevision`, tagged `value`, `requestId` → existing stored/computed mutation outcome |
| `engine_run` | Pipeline instance ID, `requestId` → admission receipt, not run completion |
| `engine_run_status` | run ID → existing durable run status |
| `engine_cancel_run` / `engine_resume_watch` | target ID, `requestId` → existing authorized control outcome |
| `live_inspect` | exact `target` → host-provided VM snapshot, dirty/edit revision and loaded package revision |
| `live_execute` | exact `target`, `expectedEditRevision`, JavaScript `source`, bounded `timeoutMs`, `requestId` → execution receipt/outcome and resulting edit revision |
| `live_reload` | exact `target`, `expectedEditRevision`, `expectedAssignmentRevision`, `discardDirty`, `requestId` → old/new live IDs, loaded revision and outcome |
| `operation_status` | `requestId` → recorded outcome only; never dispatches work |
| `audit_list` | authorized target filters, `cursor?`, `limit?` → sanitized records |

`target` is `{clientId,slotId,liveInstanceId}`; mismatched combinations fail.
Registration/heartbeat are trusted host APIs, not arbitrary agent identity claims.
Client-list and assignment results expose only targets permitted to that principal.
Inspection returns a bounded snapshot without executing caller-supplied expressions.
Execution uses the existing VM bridge: no DOM, Android Views or UI compiler access.
Subscription delivery may coalesce samples, as in P2; it is not a durable event log.
Leases expire on inactivity and are restored explicitly after reconnect. Release
owned subscriptions when the MCP session ends; no leaked background subscribers.

Structured errors distinguish `invalid_input`, `validation_failed`, `conflict`,
`forbidden`, `not_found`, `target_unavailable`, `stale_instance`,
`dirty_ack_required`, `limit_exceeded` and `internal_error`. A conflict includes
current authorized revisions so the caller can reread. An operational record has
`pending`, `running`, `complete`, `failed`, `cancelled`, `timed_out` or `unknown`
status as applicable. Pipeline admission and its run have separate identities and
outcomes. `unknown` is uncertainty about completion, never authorization to retry.

Effectful requests use a stable principal-scoped request ID and argument digest.
An exact duplicate returns its existing record; changed arguments fail. Status
queries survive reconnect. Do not redispatch a previously admitted live command,
including after server restart. An interrupted pre-dispatch request can be failed;
if dispatch might have occurred, report unknown until reconciliation proves more.

## Live execution and reload lifecycle

Admission checks permission, exact target, host availability, foreground lifecycle
and expected edit revision. Paused or disconnected targets fail immediately; there
is no deferred queue waiting for resume. The host checks the same conditions when
it receives the command, so a stale registry observation is not enough to execute.
A host delivery timeout produces a bounded failure/unknown outcome, never a command
that executes much later. Failed dashboards reject VM execution but a connected,
foreground host may inspect retained diagnostics and perform the dedicated reload.

`live_inspect` does not mark dirty. `live_execute` conservatively marks dirty and
advances edit revision when execution starts, even if supplied code only appears to
read or later throws: arbitrary JS cannot be proved side-effect free. Normal authored
UI interaction does not mark dirty. Edit revision guards agent edits/reloads, while
the VM's existing state revision guards state commits. Async work can interleave;
a stale VM snapshot fails rather than blocking the event loop across I/O.

On pause, replacement or cancellation, skip unstarted live work and fence subsequent
commits/dispatches from cancelled invocations. Already performed engine effects
survive. Dropping an MCP response is not proof that execution stopped. Reconcile the
request ID; never resend as a new action merely because the response was lost.
Explicitly started server Pipelines retain their server-owned lifetime.

Reload is a dedicated host operation, usable even when guest code has failed. It
checks the live ID and edit revision at replacement time. A dirty target requires
`discardDirty:true`; omission/false fails without discarding anything. If an edit
starts after inspection, the revision guard makes an older acknowledgement stale.

Resolve the desired assignment and the latest coherent saved package once, then
pin that package for the reload. A later save can immediately make update available
again; it cannot splice new definitions into the package being loaded. If the
assignment changes during preparation, fail conflict instead of loading a different
selection. Obtain and validate the package before retiring the old runtime where
possible. Preparation failure leaves it intact. A startup failure after replacement
leaves a visibly failed new instance with a new ID and diagnostics; do not hide the
failure by reporting successful reload.

Replacement cancels old local invocations, releases their subscriptions, discards
temporary code/state, restores authored initial behavior and creates a new live ID.
A lost reload response is reconciled through its request record and registry; an old
ID cannot reload the replacement again. Reload never rolls back server effects.
Local offline Reload/Restart can use the scoped cached baseline and must identify
that revision as cached. MCP reload of a disconnected target fails immediately;
it cannot promise adoption of a latest revision while offline.

## Contract verification

TALIA-38 is documentation only. Check links and consistency with existing contracts;
do not change frozen P0 inputs or relabel P1–P3 evidence as P4 verification.
Subsequent implementation qualification must demonstrate:

- Conflicting saves do not overwrite; invalid bundles leave every definition,
  package and engine activation unchanged; committed sources survive restart.
- Shared-reference changes affect all saved dependents while loaded clients retain
  their coherent revisions until reload; parameter edits remain instance-local.
- Two browser tabs share registration but keep independent selections and live IDs;
  native Android follows the same assignment and lifecycle rules.
- Permission families remain independent; injected calls cannot exceed agent/host
  grants, and audit records never expose credentials or arbitrary raw payloads.
- Paused/disconnected/stale targets reject commands, delayed deliveries never run
  after resume/reload, and ambiguous outcomes are reconciled without replay.
- Inspection stays clean; execution marks dirty; dirty reload requires current
  acknowledgement; failed guests can be restarted through the host operation.
- Reload leaves other clients, saved definitions and committed server effects intact,
  releases old subscriptions and reports the new live ID and actual loaded revision.
