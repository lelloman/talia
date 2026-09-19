# Server engine

Status: decisions recorded from discussion, 2026-09-18. This defines the agreed
model and implementation direction, not finalized schemas or runtime semantics.

## Ownership and implementation

The engine lives on the server. DataSources, Pipelines, Variables and Watches
are server definitions and state. Monitoring continues with no clients connected.
The server is implemented in **Rust with Axum**, with an embedded **JavaScript
runtime for configurable Pipeline and Watch logic**. The runtime and isolation
mechanism remain to be selected. Scripts access explicit engine capabilities;
using JavaScript does not give them unrestricted server access.

The client's “engine” is an API/SDK for this server, exposing read, write and
subscribe operations. Clients own their renderer, JavaScript ViewModel and local
UI state, not authoritative monitoring state. MCP can access the engine directly;
MCP manipulation of a live ViewModel targets a particular client instance.

## Four primitives

| Primitive | Responsibility |
|---|---|
| DataSource | Connection to where data originates, including connection settings and credential references |
| Pipeline | Work that collects or transforms data and produces Variables |
| Variable | Named, typed result available to dashboards, Watches and other Pipelines |
| Watch | Stateful monitoring logic that observes Variables, remembers prior events and invokes actions |

A DataSource does not itself define collection timing or processing. Multiple
Pipelines can use one DataSource with different queries or schedules. An HTTP
endpoint can be a DataSource; the act of probing it is Pipeline work. Derived
Pipelines can consume existing Variables without an external DataSource.

```text
Prometheus DataSource → collection Pipeline → CPU / memory / disk Variables
                                              ↓
                                      derived Pipeline
                                              ↓
                                       derived Variable
```

## Variables and computed values

Every Variable has a declared type, a latest value (when available), a timestamp,
and quality/status information. History retention is configurable per Variable;
the storage policy and retention limits remain open. Values can be scalar or
structured, including objects, lists, tables and time series. Exact schemas still
need definition. An unevaluated value must be distinguishable from a legitimate
`null` result; unavailable or restored data must not appear freshly measured.

A stored Variable holds a value published by a producer such as a Pipeline.
A **computed value** exposes a getter and an optional setter, with its own
internal state and an injected time provider available to both functions.
It can read other Variables, query an underlying source without exposing
intermediate Variables, perform computations, or combine these operations.
Underlying sources still use configured access and permissions.

Computed values are not restricted to pure or stateless getters. Their internal
state can contain a cached result, expiry time, incremental-fetch cursor or
accumulated data. Server-side computed state is persisted. Lazy behavior is an
implementation of this model, not a separate Variable kind or required lazy flag.
A getter may calculate every time or reuse a cached result according to its logic.
Persisting internal state does not require every computed result to have a separate
backing field. Result history, when enabled, is distinct from an internal cache.

For example, a cache getter can check expiry and capture a state/configuration
revision, then await a source read. Other operations may change that instance
while the read is pending. Before publishing the new cache value and expiry, a
short synchronous guarded update must check that the captured revision is still
current and the operation has not been cancelled. Helper APIs and stale-result
handling are still to be specified; an unconditional assignment after `await`
would not provide this guarantee.

The optional setter can update internal state or request an authorized engine
operation. Reads/evaluations support asynchronous work and explicit errors.
Exact setter/result contracts, state serialization and failure handling remain
open. Directly writable stored Variables and producer ownership rules have not
yet been decided.

### Dependencies and subscriptions

Computed values declare their Variable dependencies. Talìa uses those declarations
to re-evaluate subscribed computed values when dependencies change. Values backed
by external sources also support explicit refresh or invalidation.

Invalidation marks a cached result as outdated; refresh requests evaluation.
The mechanism connecting these operations to a getter's arbitrary internal cache
still needs definition. Declaring dependencies does not automatically discover
hidden source changes or specify polling intervals. Cycles must fail immediately;
detection coverage, propagation ordering, event coalescing, result equality, error
notifications and evaluation when there are no subscribers remain open.

### Async execution and atomic updates

Normal JavaScript async interleaving is the agreed execution model. A single event
loop per runtime is sufficient; synchronous execution runs until it yields. A
getter, setter or `read` can itself await I/O or other computed reads. At `await`,
other operations may run **on the same instance** as well as other instances.
Talìa must not hold a per-instance execution lock or queue slot across that wait.
This replaces the earlier whole-operation serialization decision.

Only short synchronous state updates are atomic. A check of the current state or
revision and its corresponding update belong in the same atomic section, which
cannot contain `await`. An operation resuming after an await must account for
state, parameters or definitions having changed. Provide helpers for versioned
commits and conflict handling rather than requiring every author to implement
them independently. Exact revision semantics and discard/retry policies remain
open; an obsolete result must not silently overwrite newer state.

Server state updates are coordinated at the authoritative engine across all
clients. Frontend-local state has its own atomic-update boundary within that
frontend. A client-side `await read` followed by `await write` is not an atomic
read-modify-write operation; the engine must provide the appropriate guarded
update capability. These guarantees do not imply multi-Variable transactions,
rollback, crash-atomic persistence or atomic external effects. The durable commit
and API contracts still need definition.

### Concurrent read policy

Each computed value can configure one of two policies:

- **Shared refresh:** concurrent readers await the same in-flight getter execution.
  This is useful for cached metrics or expensive probes.
- **Independent reads:** each read starts its own getter execution.

Sharing deduplicates a getter execution; it does not lock the instance. Setters,
invalidations and other operations can proceed while it awaits. Ordinary access
to the getter's own local state is synchronous, even though fetching a computed
value through `read` can await. Whether a read returns a cached value immediately
or awaits refresh is a separate freshness policy, still to be specified.

The default mode, join key/parameter compatibility, invalidation during a shared
refresh and cancellation of one subscriber to shared work remain open. Sharing
must not accidentally cancel work still required by another reader.

### Cycles, cancellation and recovery

Reject a dependency cycle immediately rather than waiting for deadlock or
silently substituting an old value. The dependency discovery/error API remains
to be specified.

Skip cancelled work that has not started. Cancellation of running work does not
block other operations on the instance while I/O is outstanding. Fence subsequent
state publication/commits and new effect dispatch from the cancelled operation,
including its late completion. Do not undo state changes or external effects
already dispatched. Underlying I/O may still finish; remote cancellation,
deadlines, shared-work cancellation ownership and recovery details remain open.

The [execution-policy record](execution-policy-prototype.md) distinguishes these
agreed decisions from the superseded serialized prototype. That prototype and its
stored results require replacement tests before they qualify this contract.

## Pipeline execution

Pipelines support three execution modes:

- **Automatic:** ongoing or scheduled collection/processing, such as host stats.
- **Triggered:** execution requested by monitoring conditions and stateful Watches.
- **Explicit:** execution requested through the engine by a client or MCP caller.

Execution triggers are separate from Pipeline logic. The same investigation
Pipeline can run from a Watch action or an explicit request. Scheduling syntax,
input-change execution, concurrency, retries and missed-run policy remain open.

For example, automatic collection maintains disk-space Variables. A Watch can
then invoke a Pipeline that measures disk usage by service and publishes the
result as another Variable. This is an example, not a built-in disk workflow.

## Stateful Watches

A Watch has internal state as well as inputs and actions. It can remember that
one condition occurred before taking action on another condition. JavaScript
expresses the configurable logic; flags or state-machine representations are
possible, with the exact programming API still undefined.

Illustrative behavior:

```text
Available space crosses below 10% → remember warning condition
Available space later crosses below 6% → remember investigation condition
                                      → invoke investigation Pipeline
```

Watch state is durable and independent of clients. A service restart does not
erase that memory, and dashboard reload/disconnection does not reset it.
Client-side subscription reactions are local ViewModel behavior, not server
Watches, even though both use JavaScript.

Reset/recovery conditions, repeated actions, missing data, and observations that
cross multiple boundaries at once still need explicit semantics. The example
above does not decide what happens when space jumps directly from 12% to 5%.
Likewise, persistence does not by itself guarantee exactly-once external actions;
state/action recovery and duplicate prevention need design.

## Shared definitions and instances

A reusable Watch definition declares behavior and configurable parameters.
Instances reference that shared definition, supplying their own input/source
bindings and thresholds, with independent persisted state.

```text
WatchA(source/input, highThreshold, lowThreshold)
├── Disk X: source X, 10%, 6%, own state
└── Disk Y: source Y, 15%, 8%, own state
```

Updating WatchA updates the behavior referenced by both X and Y. Updating X's
parameters affects only X. Neither instance shares mutable state with the other.
These are references, not independent copies of the definition. The concrete
source-to-Variable binding syntax remains open.

This reference model also applies to UI elements, ViewModel elements and
functions. Definition edits propagate to references; instance parameters and
state remain separate. Function execution capabilities depend on its context;
reuse does not imply identical server/client privileges or shared mutable globals.

How updates are activated, how in-flight executions are treated, and how state
is migrated when shared logic changes remain open. For clients, saved definition
updates remain persistent authoring operations; they do not bypass the prohibition
on live MCP edits to View definitions. Timing of adoption by running clients is
still undecided.

## Runtime configuration without service restart

Minimizing downtime is a design goal. DataSource, Pipeline, Variable and Watch
definitions can be created, updated and removed while Talìa runs, through
MCP-backed engine operations. Adding source data or derived values must not
require restarting the server. These are persistent engine configuration changes,
not temporary dashboard dirty state.

Validate changes before activation, including references, types and dependencies.
Invalid changes leave the previous active definition in place. Affected Pipelines
and subscriptions must be updated while unrelated monitoring continues. Report
incompatible dependencies rather than silently breaking them. Exact activation,
rollback, deletion and multi-definition transaction behavior still need design.
This requirement concerns supported runtime definitions; it is not a promise
that arbitrary server binary changes need no restart.

## Persistence and recovery

Persist both engine definitions and engine data/state:

- DataSources, Variables, Pipelines, Watch definitions and instance parameters.
- Stored values, configured result history, timestamps and quality/error information.
- Watch instance state, computed-value internal state, and recovery checkpoints.

On restart, load definitions, restore retained state/data and resume processing.
Recreate live subscriptions and execution machinery rather than treating them
as serialized running processes. Retained values keep their original timestamps.

A database is required. **SQLite is a candidate, not a finalized storage choice.**
History retention, value storage strategy, transactions, recovery guarantees,
backups and schema migration remain to be specified. Talìa is not committed to
copying all Prometheus history into its own database.

## Open details

- Exact Variable schemas, producer ownership, history policies and writable inputs.
- Computed getter/setter/state/time-provider APIs, dependency updates, refresh and
  invalidation, state persistence on failure, and recursive/concurrent execution
  edge cases. Async interleaving, short atomic updates and configurable shared versus
  independent reads are decided; their public APIs and conflict policies are not.
- DataSource adapter interface and credential management.
- Pipeline programming API, output publication, dependency cycles and execution.
- Watch input evaluation, state API, recovery/reset rules and action delivery.
- Definition activation, dependency validation, shared-definition updates and
  state migration, including deletions and in-flight work.
- JS runtime selection, server/client compatibility, bridges and execution limits.
- Engine API signatures, permissions, stream semantics and persistence technology.
