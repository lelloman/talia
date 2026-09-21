# MCP live dashboard control (P4)

[TALIA-45](https://crumbles.lelloman.com/w/LLPR/TALIA/45) adds authenticated client
discovery, inspection, temporary execution and dedicated reload to the existing
[stdio adapter](mcp-authoring.md), on both web and native Android. Saved authoring
and direct engine access retain their separate permission families.

## Tool interface

Every live target is exactly `{clientId, slotId, liveInstanceId}`. Discover it with
`clients_list`; display names do not confer identity or authority.

| Tool | Arguments | Result |
| --- | --- | --- |
| `clients_list` | Optional `clientId`, `cursor`, `limit` (1–50, default 25) | Authorized registrations/slots, timestamped status, `nextCursor` |
| `live_inspect` | `target` | Host snapshot with tagged root state, dirty/revision/failure information and loaded package metadata |
| `live_execute` | `target`, `expectedEditRevision`, `source`, `requestId`, optional `timeoutMs` (100–5000, default 5000) | Durable audit and host lifecycle outcome |
| `live_reload` | `target`, `expectedEditRevision`, `expectedAssignmentRevision`, `requestId`, optional `discardDirty` | Audit, old/new live IDs, actual package/edit revision |
| `operation_status` | `requestId` | Recorded audit and available lifecycle outcome; never re-executes |

Inspection does not run an agent expression and does not mark dirty. Failed guests
retain their latest available state and a safe failure diagnostic for inspection.
Source and arbitrary exception text do not enter audit records. Listings filter
slots individually, and opaque cursors bind the principal, credential, policy and
query. Status is last-known observation, not a guarantee that a command will run.

## Temporary JavaScript

`source` is the body of an async function receiving `ctx`. A separate bounded
QuickJS invocation context talks to the loaded root ViewModel through host methods:

```js
const snapshot = ctx.state();
await ctx.commit(snapshot, {...snapshot.value, details: false});
const sample = await ctx.read("value");
await ctx.write("value", sample.value + 1);
```

`ctx.state()` returns a copied `{revision, value}` snapshot, initially captured from
the loaded VM and updated after this invocation's successful commits.
`await ctx.commit(snapshot, next)` checks the real VM revision before changing its
state. Authored actions/subscriptions can interleave during awaits, so stale commits
fail rather than holding an execution lock across I/O. `ctx.read(id)` returns a
public engine sample; `ctx.write(id, value)` writes a stored Variable;
`ctx.run(id)` admits a server Pipeline. Engine values and inspection state preserve
undefined, NaN, infinities and negative zero through the existing tagged codec.
Execution returns a receipt; inspect explicitly to read the resulting VM state.

This first live API provides invocation-local logic and temporary root-state edits.
It does not install persistent callbacks or replace loaded handlers. Reusable
handler/function changes use saved authoring and explicit reload. This keeps an
invocation's authority and lifetime explicit. The invocation has no DOM, Android
Views, UI compiler, loaded VM globals, registration credentials or server tokens.
Host state commits target the existing VM; Views and saved sources are immutable.

Execution marks dirty and increments the host edit revision once when it starts,
even for read-only-looking code or subsequent failure. Edit revision is distinct
from VM state revision. Normal UI actions remain clean. An internal live-script
failure stops the dashboard visibly; cancellation stops the invocation and preserves
prior commits/effects. Reload remains available through the host when guest code
has failed.

## Delivery, authority and recovery

Commands use authenticated host requests under `/clients`. Hosts poll every 200 ms
on web and on the existing Android tick. At most one command per exact live target
and 64 total can be pending. A command is delivered at most once and expires within
its deadline; it is never queued for a disconnected/paused target to resume later.
Both server admission and receiving host check identity, foreground state, edit
revision and dirty acknowledgement. Host checks resolve races with last-known
registry reports; stale reports cannot authorize execution.

The host starts an admitted command before effects. Every asynchronous continuation
rechecks its deadline, local lifecycle/identity and the server capability. Engine
calls require the live execution permit, current agent engine permission and the
loaded package's host grants. Capabilities stay outside guest state. Effect sequence
IDs reject repeats; a command permits at most 128 engine dispatches. Pausing,
replacement, revocation, MCP cancellation or connection closure prevents later
operations. A caller losing its response cannot infer that an earlier effect failed.

SDK cancellation is forwarded using private connection/call identities. Early
cancellation and closed-session tombstones are bounded to 256 records for 60 seconds.
At that bound, active cancellation still works and new live requests fail closed
for 60 seconds rather than forgetting a cancellation.
Live source is limited to 64 KiB, host results to 256 KiB and outstanding invocation
calls to 32. The existing QuickJS limits remain 16 MiB heap, 512 KiB stack and a
500 ms CPU turn budget; browser workers also have a three-second host watchdog.
No live invocation or SQLite transaction blocks the event loop while waiting on I/O.

Reload prepares and validates one coherent saved package before retiring the old
runtime, rechecks assignment/edit revisions and requires `discardDirty:true` for a
dirty target. It closes old subscriptions, restores authored state/behavior and
registers a new live ID. Preparation failure preserves the old runtime. Startup
failure after replacement leaves the new instance visibly failed. Other client
slots and committed server effects are unaffected. MCP reload requires connectivity;
local offline restart retains its existing cached-baseline behavior.

SQLite schema 9 adds `live_outcomes`: lifecycle metadata keyed by audit ID, committed
with completion. It stores no VM snapshot, source or script return value. Exact
request-ID retries return the prior record; changed arguments conflict. Restart
marks unresolved running work unknown and never redispatches it. If host dispatch
may have happened but completion cannot be proved, an unknown outcome is intentional.
The registry and `operation_status` support reconciliation, not automatic replay.

## Verification

```sh
cargo test --manifest-path engine/Cargo.toml --locked --offline
cargo build --manifest-path engine/Cargo.toml --locked --offline --bins --examples
node --test dashboard/tests/vm.test.mjs dashboard/tests/compiler.test.mjs
cargo test --manifest-path dashboard/native/Cargo.toml --offline
bash dashboard/build-web.sh
bash dashboard/build-android.sh
python3 engine/tests/emulator.py  # separate terminal: disposable emulator-5570
python3 engine/tests/mcp_live.py --android
adb -s emulator-5570 emu kill
```

The external test drives the real stdio adapter and both hosts. It covers clean
inspection, independent tabs, tagged state, engine permission intersection, revision
and dirty guards, deduplication, cancellation after an effect, failed-guest restart,
new reload identity and paused rejection. Rust tests cover early cancellation,
expired delivery, host ownership, effect replay, revocation and audit failure.
Existing authoring, engine and client lifecycle suites remain regressions.
P4 end-to-end qualification is TALIA-46; deployment and embedded chat are outside P4.
