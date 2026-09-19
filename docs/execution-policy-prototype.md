# Execution policy decisions and async experiment

Status: **async interleaving agreed; revised fixture tested on all four hosts.** The
[engine model](engine.md#async-execution-and-atomic-updates) is authoritative for
these decisions. The remaining API details and overall detailed specification are
not signed off.

## Current decisions

- Use ordinary JavaScript event-loop behavior. Reads, getters and setters may
  await; other operations on the same instance can run meanwhile. Do not retain
  an exclusive per-instance queue slot while awaiting I/O.
- Make only short synchronous state updates atomic, with no `await` inside the
  update. Resumed operations must account for changed state/configuration before
  committing. Guarded-update APIs and precise conflict policies remain to be designed.
- Configure each computed value for shared refresh (readers join one in-flight
  getter) or independent reads (each read executes a getter). Sharing is not a
  lock: setters and other operations remain able to run. Read mode must be explicit. One reader cancellation stops only that wait; the last cancels the producer. Cache freshness and invalidation APIs remain open.
- Fail dependency cycles immediately. Skip cancelled work that has not started.
  Prevent running cancelled operations from making subsequent commits/publications
  or new effect dispatch, including after I/O completes. Cancellation does not hold
  the instance or undo effects already dispatched.

The earlier agreement to retain a running operation's queue slot across await was
explicitly revised. Cycle failure, skipping cancelled work and no rollback remain;
the non-interleaving guarantee for an entire async operation does not.

## Current candidate API and evidence

The shared fixture now has no per-instance operation queue. `start(key, action)`
and `read(key)` schedule ordinary Promise jobs; suspended operations do not block
same-instance getters, setters or invalidation. `define` explicitly selects
`shared` or `independent` reads. There is no implicit default mode or cache policy.
These helper signatures and the policies below are **prototype choices**, not
public API commitments. The subsequently approved [runtime contract](runtime-contract.md) now requires explicit read mode, stale failure without retry and per-reader cancellation ownership.

Each instance owns JSON state and a revision. `snapshot()` returns a detached copy
with an internal revision stamp. `commit(snapshot, nextValue)` validates and copies
plain JSON, checks the stamp and replaces the state synchronously. It accepts a
value, not an update callback; Promise values and accessors are rejected. No await
can occur inside the replacement. A successful commit advances the revision;
other operations based on the previous revision become stale. Snapshot reuse is
rejected. Definition replacement and explicit invalidation also advance the
revision. In this fixture, definition replacement resets state to the supplied
initial value; production state migration remains undecided. Resumed operations must pass cancellation/revision checks before reads,
commits, effect dispatch or successful result publication, including Promise adoption jobs between callback
completion and delivery to readers. Falsy thrown errors remain rejections. This candidate rejects
stale work rather than automatically retrying or merging it.

Shared reads join one current in-flight getter. They wait for its result; the
helper does not implement stale-while-revalidate or persistent cached results.
Setters can run during shared refresh. Mutation/invalidation detaches the old
refresh so a new read can start immediately. Each reader has its own cancellation
lease. Cancelling one reader rejects that reader immediately; cancelling the last
reader marks the producer cancelled and releases its dependent leases. A child
shared with another caller survives parent cancellation. Already dispatched I/O
may continue physically, but its old continuation cannot use guarded operations
or publish a successful result. Earlier commits and dispatched effects survive.
Settled contexts cannot dispatch detached effects.

The helper detects recursive instance reads and tracks actual task wait edges,
including joins between independently started shared roots. No FIFO predecessor
edges exist. Cycle admission fails immediately. Completed tasks release their
readers and edges; cancellation does not promise to terminate arbitrary JavaScript
or physically cancel external I/O. Host retirement remains the fallback for
non-cooperative code.

On 2026-09-19, Linux, Chromium, capped browser Workers, Android x86_64 and physical
ARM64 passed the revised 14 behavior checks plus 27 execution checks, over 20 fresh
contexts each. Linux child replacement and Android service rebind rerun the suite.
Coverage includes overlapping independent reads; setters during suspended getters
and shared refresh; competing commits; detached state copies; rejected async
updates; stale results after invalidation/definition replacement; shared-reader
and dependency cancellation; immediate cycles; preserved effects; and cleanup.
`spikes/runtime/check-results.py` requires matching reports across hosts.
The physical test app was stopped and uninstalled after collection.

## Remaining work and limits

Choose the production API, cache freshness and definition state migration explicitly. Read mode has no default; stale evaluations fail without automatic retry, as recorded in the runtime contract.
The trusted helper is not authoritative server scheduling or a security boundary:
arbitrary guest JS can bypass it by mutating its own objects or calling the raw
fixture engine bridge. It sees context-mediated reads, not arbitrary Promise or
external dependency graphs. It does not implement durable transactions, automatic
dependency invalidation, remote cancellation acknowledgement or crash recovery.
Single-event-loop atomic replacement does not establish cross-client/database
atomicity. Production enforcement, backpressure and external action outcomes remain
open in the [implementation plan](implementation-plan.md).

## Historical record

The following describes the former implementation only. Its unchanged reports and
input hashes are archived in
[`results/history/serialized-2026-09-19`](../spikes/runtime/results/history/serialized-2026-09-19/README.md).
The active fixture and top-level result files now exercise async interleaving.

## Superseded prototype behavior

`ExecutionScheduler` in the runtime fixture maintains a FIFO queue per instance
and a graph of operations waiting for other operations. Queue predecessors are
part of that graph, as are dependency reads made through the operation context.
A read that would close a cycle rejects before admission. This catches direct
recursion, indirect recursion and cycles between separately started root operations.
Acyclic reads sharing a dependency are allowed. Rejection releases graph edges so
subsequent work can run.

A task handle exposes a Promise and cooperative cancellation. Cancellation marks
the task and its current child dependency tasks; further dependency reads are
rejected. A cancelled queued callback is skipped when its queue slot arrives.
Running work keeps its slot until it settles, preventing a replacement getter or
setter from interleaving with its suspended continuation. Independent instance
queues continue running.

The trusted callback context offers a cancellation check, guarded synchronous
state commits and guarded effect dispatch. Cancellation before a guarded commit
prevents that mutation. Cancellation after an effect has been dispatched does not
undo it: the test writes an engine value, cancels the caller and verifies that the
write remains visible. Earlier state commits also have no rollback guarantee.
After cancellation settles, the next queued operation can run normally.

## Historical evidence

Thirteen shared checks run alongside the original fourteen behavior checks over
20 fresh contexts on Linux, Chromium, the Android x86_64 emulator and the physical
ARM64 phone. The capped disposable browser Workers run the same checks. Reports
record these separately in `execution`; `check-results.py` requires matching results.
Linux child replacement and Android service rebind also rerun the native suite.

The checks cover direct, indirect and concurrent-root cycles; valid shared
acyclic dependencies; recovery after rejection; queue retention during running
cancellation; independent progress; rejected late commits; skipped queued work;
cancellation propagation to dependencies; recovery after cancellation; preserved
engine effects; and final graph/queue cleanup.

## Limits of the historical experiment

This is a trusted JavaScript fixture helper, not server-owned scheduling across
clients. Direct state mutation can bypass its commit guard. It observes only reads
made through its context, not arbitrary Promises, hidden external dependencies,
static definition graphs or subscription/invalidation propagation. Detached work
and misuse of asynchronous commit callbacks are not qualified.

Cancellation does not forcibly stop a callback or cancel a remote operation. A
non-cooperative task can hold its queue indefinitely until a host deadline retires
its runtime; the separate Worker/process experiments test that containment and
replacement mechanism. Deadline policy, remote cancellation acknowledgement,
action status after disconnect, retry/idempotency rules and durable recovery still
need design and tests. Local queue recovery is not crash-atomic database recovery.

These archived passing checks are not current concurrency acceptance.

## Protected execution authority (TALIA-12)

`spikes/runtime/host/execution.js` now owns instance revisions, required read modes,
producer/reader leases, dependency edges, cancellation and result settlement in a
trusted realm outside authored code. Browser parents run it outside the Worker;
native hosts use a separate QuickJS context which is never exposed to dashboard
scripts. The host binds an operation to a guest; guest messages cannot choose its
operation identity. Dispatch rechecks that binding before any bridge operation.
Protected state read/commit/result operations address the bound instance only.

Fresh `results/protected-execution` reports record matching host-authority tests and
real raw-bridge stale/cancelled effect rejection on Linux, Chromium and Android
x86_64, alongside the existing 27 asynchronous checks. Protected state access and
preservation of previously dispatched effects also pass. This adds an enforcement
boundary; the old in-guest helper remains a behavioral fixture, not that boundary.
The operation-per-guest binding is an evaluation adapter, not a finalized public
API or a production multi-client scheduler. Physical-device requalification remains
TALIA-15; persistence and server restart recovery remain P2.
