# Execution policy experiment

Status: tested candidate, 2026-09-19. **Not a signed-off engine contract.**
The [engine specification](engine.md#serialization-and-atomicity-boundary) defines
per-instance serialization; its cancellation, failure and dependency semantics
remain open. This experiment provides a concrete option to review.

## Candidate behavior

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

## Evidence

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

## Limits and decisions still needed

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

Before implementation becomes authoritative, decide the public cancellation/error
contract and enforce scheduling and mutation guards at the engine boundary.
