# Computed execution

The server uses bounded QuickJS contexts on a Tokio local executor. Async waits
release execution to other instances and other operations on the same instance.
Each invocation stages `ctx.state`; successful completion commits state and result
with captured instance/dependency revisions. `await ctx.commit()` explicitly commits
private state earlier. Failure discards only staging since the last explicit commit.

An object expression defines `get(ctx)` and optional `set(ctx,value)`. Context offers
`params`, mutable `state`, `changed` dependency IDs, injected `now()`, `read(id)`,
`write(id,value)`, `commit()` and bounded `sleep(ms)`. Reads must target declared
dependencies; writes additionally require setter context and stored destinations.
No network, filesystem, credentials or platform globals are exposed. Collection
source adapters are later P3 work. State-schema validation runs before each commit.

Concurrent reads share a detached producer by default; independent policy is
explicit. Caller timeout/drop only stops that wait, including the final caller.
A producer has a five-second total deadline; synchronous turns have a 100ms CPU
budget, 16MiB heap and 512KiB stack. Host calls are bounded. Dependency cycles are
rejected at definition activation and undeclared dynamic reads fail.

Stale getters retry within the original producer deadline; individual reader
waits keep their own deadlines. Setters never automatically replay. Definition or
parameter generation replacement cancels old work rather than retrying old code.
Revision guards execute after awaits and before every state/effect dispatch.
Already committed effects survive subsequent cancellation or script failure.

Invalidation marks metadata and preserves private state; `changed` informs the
next getter. Subscribers cause reevaluation; otherwise work waits for a read.
Latest value/age/quality and evaluation running/error/invalidated metadata are
separate, leaving presentation to widgets. Successful undefined/NaN are values.

Explicit invalidation advances the instance revision without changing private cache fields or measurement age, fencing in-flight cache publication. Recovered computed instances begin invalidated until reevaluated. Definition migration batches have a combined 100ms script budget; v1 bounds configuration at 256 definitions and 1024 instances.

Authenticated [MCP setters](mcp-engine.md) add a trusted host guard to this lifecycle.
They recheck primary setter authority before continuations, commits and effects,
and nested resource read/write authority before dispatch (and after awaited reads).
Cancellation fences subsequent effects without undoing earlier commits. The guard
is never supplied by guest code and does not serialize I/O across instances.
