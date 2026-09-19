# Transport prototype findings

Status: shared client–server transport fixture passes on Linux, browser, Android
emulator and physical ARM64. This is evidence for candidate behavior; public API
policies and full P0 qualification remain open.

The [runnable experiment](../spikes/transport/README.md) connects QuickJS hosts to a
real loopback Rust/axum server. The same JavaScript adapter and tests run on all
four hosts. Network I/O runs outside the JS event loop, and the Android/browser UI
continues updating a heartbeat. The server holds no state mutex over awaited I/O.
It implements only an in-memory numeric value and action records, not Talìa's
DataSources, Pipelines, Watches or persistence.

## Results and proposed semantics

Twenty-five shared checks cover concurrent reads/writes during delayed network I/O;
revision ordering; live subscriptions and unsubscribe; reconnect snapshots and
resumed subscriptions; pending-read rejection; generation isolation with colliding
IDs; cancellation before dispatch, before effect and after completion; uncertain
action outcomes after actual HTTP response loss; reconciliation; idempotent retries
and argument conflicts; independently completing accepted work; and cleanup.
Ten server checks additionally cover malformed/oversized input, range/field
validation, namespace validation, concurrent duplicate actions and recorded failure
on a pre-effect timeout.

The useful distinction is between **the caller stopping its wait** and **the
server stopping an accepted action**. A cancellation request can arrive after an
effect; the reply then reports completion and no rollback occurs. Similarly,
losing an HTTP response leaves the caller uncertain. Stable action IDs and a status
lookup let it recover an outcome when the server still has the record. The client
does not automatically replay mutations. Unknown records stay unknown.

State delivery uses revisioned snapshots. Old data cannot replace a newer observed
revision, and responses from previous client generations cannot resolve replacement
requests, even if their numeric IDs match. A delayed successful action response
still reports success while its older snapshot is ignored. Reconnection starts
from the current snapshot and resumes listeners; intermediate updates may coalesce.
This does not provide durable event/alert delivery or replay history.

The stored reports are [Linux](../spikes/transport/results/linux.json),
[browser](../spikes/transport/results/browser.json),
[Android emulator](../spikes/transport/results/android.json) and
[physical Android](../spikes/transport/results/android-arm64.json).
The [validator](../spikes/transport/check-results.py) checks cross-host parity and
source hashes. Runtime behavior and Android process-recovery regressions also pass.

## Limits and next step

Wire/API details remain prototype choices. The subsequent [runtime contract](runtime-contract.md) requires resume reconciliation without resubmission and preserves already-dispatched effects. The test
uses a logical client generation change; actual runtime replacement is covered by
the separate [runtime experiment](runtime-prototype.md). There is no production
renderer yet. Android uses a native test Activity/JNI, not a finished Kotlin client.

The server is local, unauthenticated and deliberately exposes test fault controls.
Its bounded in-memory ledger is neither a durable transaction log nor a guarantee
for external operations. Server restart loses records/state; incarnation negotiation,
automatic reconnect/backoff, authorization, production network adapters and Android
background behavior remain unqualified. The web and native HTTP implementations
are test hosts, not a final transport selection.

The next useful increment is the shared UI/VM vertical slice: compile a minimal
restricted JSX-like definition and render the same bound text, controls and screen
navigation in a DOM client and a native Android client. Connect those controls to
the tested engine boundary. Keep the remaining transport/persistence decisions
visible rather than interpreting this experiment as completion of P0.
