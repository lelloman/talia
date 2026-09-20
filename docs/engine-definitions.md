# Runtime definition activation

Definitions have an ID, increasing version, stored/computed kind, value/state
schemas, declared dependency instance IDs and shared/independent read policy.
Computed source is an object expression with `get(ctx)` and optional `set(ctx,value)`.
Instances reference the definition and own parameters, state and revisions.

Updates use an expected version. Validate source in a bounded capability-free
QuickJS context, check dependency references/cycles, prepare all optional synchronous
state migrations, then commit the definition and every affected instance together.
A migration receives `(state, parameters)` and returns new state; promises are
rejected. No migration I/O is allowed. Failure leaves all previous rows intact.
The instance generation and revision advance on activation or parameter changes;
old work cannot commit against them. Prior external effects are not reversed.

Referenced definitions cannot be removed. Removing an instance referenced by
another definition is rejected. State is preserved without a migration and must
still satisfy the new schema. A narrower result schema must also accept existing
results. Parameter changes affect only the selected instance.

These are storage/activation primitives. The computed runner supplies active-work
cancellation checks and the transport supplies subscription refresh notification.
