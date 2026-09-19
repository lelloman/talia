# Execution policy decisions and historical experiment

Status: **async interleaving agreed; serialized prototype superseded.** The
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
  lock: setters and other operations remain able to run. Default mode, freshness
  policy, invalidation rules and shared-reader cancellation ownership remain open.
- Fail dependency cycles immediately. Skip cancelled work that has not started.
  Prevent running cancelled operations from making subsequent commits/publications
  or new effect dispatch, including after I/O completes. Cancellation does not hold
  the instance or undo effects already dispatched.

The earlier agreement to retain a running operation's queue slot across await was
explicitly revised. Cycle failure, skipping cancelled work and no rollback remain;
the non-interleaving guarantee for an entire async operation does not.

## Required follow-up

The implementation and results below have **not** been changed by this documentation
update. `ExecutionScheduler`, the original `serial` helper and their tests still
exercise the old model. In particular, checks for holding a slot after cancellation
and preventing same-instance interleaving are no longer desired product behavior.
Replace those tests and rerun all hosts before recording evidence for the new model.
The [implementation plan](implementation-plan.md#execution-contract-follow-up)
tracks this work, including stale results, shared refresh and reader cancellation.

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

This helper must be revised before it can qualify the current execution contract.
The old passing checks must not be presented as current concurrency acceptance.
