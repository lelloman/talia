# P0 runtime findings

Status: dependency-cycle and cancellation/recovery candidate tested, 2026-09-19. **P0 qualification is incomplete.**
Runnable code and commands are in the [runtime experiment](../spikes/runtime/README.md).
The implementation plan's full runtime gate is not passed by these smoke tests.

## Candidates tested

| Host | Implementation | Environment |
|---|---|---|
| Linux | rquickjs 0.13.0 / QuickJS-NG, Rust CLI | x86_64 Linux |
| Android | Same Rust host cross-compiled, JNI-backed native Activity and service processes | x86_64 emulator and physical ARM64 OnePlus CPH2493, Android 16/API 36 |
| Browser | QuickJS-NG 0.32.0 WASM inside Worker; Bellard 0.31.0/0.32.0 memory comparisons | Chromium 145.0.7632.6 |

Cargo/npm lockfiles pin the resolved dependencies. The Android test uses NDK
27.0.12077973, AGP 8.13.2 and Gradle 8.13. It proves native embedding without
requiring Kotlin/JS compilation. It does not select a production renderer.

The QuickJS builds are different distributions, not interchangeable bytecode
engines. Fixtures share source code. Runtime selection must retain that distinction.

## Results

The shared behavioral suite executes 20 times with a fresh context each time.
It covers typed JSON transport, host errors, async serialization, cache
concurrency, subscription delivery/unsubscribe, cancellation, late/duplicate
responses, host effects and restoration of baseline VM state. Temporary live
function and state modifications are made by the harness between reloads.

- [Linux result](../spikes/runtime/results/linux.json)
- [Browser result](../spikes/runtime/results/browser.json)
- [Android emulator APK result](../spikes/runtime/results/android.json)
- [Physical ARM64 APK result](../spikes/runtime/results/android-arm64.json)
- [Physical ARM64 service recovery](../spikes/runtime/results/android-process-arm64.json)

Linux and Android native Activity/JNI runs passed the behavior suite, infinite-loop
interruption near the configured 50 ms deadline, and the 16 MiB guest allocation
limit. Native errors did not prevent creation of a working fresh context.

Browser behavioral tests and loop interruption passed; the main page continued
updating its heartbeat. All three hosts also passed forced reload with active
subscriptions and pending calls. The host retires only the discarded generation's
resources, rejects stale responses/events even with colliding request IDs, and
keeps an unrelated instance working. This is a simulated host routing test, not
proof of cancellation of remote operations or network reconnect handling.

### Browser memory investigation

The [Node comparison](../spikes/runtime/results/memory-comparison.json) and
browser report reproduce the same gap in Bellard quickjs-emscripten 0.31.0,
Bellard 0.32.0 and QuickJS-NG 0.32.0. Each rejects one 32 MiB buffer with a 16 MiB
runtime limit, but accepts 64 retained 1 MiB buffers. QuickJS reports approximately
67 MB of used memory afterward. All three also accept 400,000 ordinary objects,
reporting approximately 38–43 MB. The earlier larger object-pressure test eventually
failed, but that did **not** demonstrate enforcement of an aggregate heap budget.
Newer string implementations can optimize the string probe; retained string count
alone is not evidence of allocated bytes. These figures are engine accounting,
not browser RSS measurements.

This matches the symptoms in upstream [issue 271](https://github.com/justjake/quickjs-emscripten/issues/271).
The local experiment does not establish the allocator's root cause. Upgrading or
switching these published distributions alone does not resolve it. The browser
report retains `heap_limit: false` and `qualified: false`.

A separate experiment supplies `WebAssembly.Memory({initial:256, maximum:256})`
through `newVariant`'s `wasmMemory` option. This caps the **whole module's linear
memory** at 16 MiB, including engine overhead. In Node and Chromium all four
allocation probes were rejected, disposal completed, growth beyond the cap was
rejected, and a separate fresh module worked. See the
[capped Node result](../spikes/runtime/results/capped-memory-node.json) and the
browser report's `capped_memory` field. The exhausted object case produced a null
exception value; callers cannot require a richly allocated error payload under OOM.

This is an alternative containment mechanism, not a repaired per-runtime limit.
Independent budgets require separate WASM modules; contexts sharing a module
share its budget. It does not cap host-side messages, queues, JavaScript memory,
Worker overhead or total RSS. The uncapped regression suite remains separate; the new `disposable` report
records the full suite and failure recovery under the module cap.

### Disposable dashboard Workers

The browser now also runs each dashboard fixture in its own Worker with its own
16 MiB capped WASM module. The parent page owns engine state, subscription records,
pending bridge operations and host timers. Retiring a generation terminates its
Worker and releases only its resources, without invoking guest cleanup.

The [browser report](../spikes/runtime/results/browser.json) records:

- Twenty fresh capped Workers passing the same 14 shared behavioral checks.
- Live state/function edits restored on reload while engine writes survive.
- Active subscription/pending-call/timer cleanup and stale-generation rejection,
  including colliding request IDs and an unrelated live dashboard.
- All four allocation probes failing within their capped modules, followed by
  parent-owned cleanup and clean replacements. Another dashboard still receives
  events and evaluates code; its pending call and timer remain registered.
- An interrupted guest loop, plus a separate trusted-harness hang that bypasses
  QuickJS interruption. The parent watchdog terminates that Worker and rejects its
  pending command. The other dashboard makes progress **during** the hang.
- The full shared suite passing in a fresh Worker after failure recovery, with no
  remaining host resource registrations when the harness finishes.

These are simulated dashboard/engine hosts, not a renderer or production transport.
The fault policy in this spike retires the Worker on an uncaught guest evaluation
error; guest exceptions caught within the shared suite remain recoverable. A null
exception under OOM also retires the Worker. The watchdog uses a 250 ms test deadline
for the injected hang and a 3 s default command deadline; these are not product SLAs.
The recovery test demonstrates fresh Worker execution, not immediate RSS reclamation
by the browser. Browser bridge validation and budgets are now tested below. The report keeps the uncapped `qualified: false` while recording
`disposable.passed: true`; neither field signs off all of P0.

### Browser bridge validation and budgets

The trusted Worker adapter validates raw guest JSON before posting it; the parent
independently validates it before applying effects. Requests require exactly
`id`, `op`, and `value`, a positive safe integer ID, an allowed operation and
operation-specific arguments. IDs must strictly increase within a generation.
The fixture permits only its `value` variable; unsubscribe requires ownership.
This is fixture capability validation, not user authentication or production ACLs.

Prototype limits (evaluation defaults, not signed-off product settings):

| Resource | Limit |
|---|---:|
| Guest request, UTF-8 including envelope | 32 KiB |
| Harness command or result envelope | 64 KiB |
| JSON nesting / visited values | 16 levels / 2,048 values |
| Worker requests awaiting parent acknowledgement | 32 |
| Outstanding parent commands per dashboard | 64 |
| Subscriptions / timers / stalled calls per dashboard | 16 each |
| Live Workers per host | 8 |

Acknowledgements replenish the Worker adapter's credits only after parent request
handling. Guest code cannot access those credits. Once the adapter detects a
violation it remains poisoned even if the guest catches the exception, and the
parent retires that Worker. Resource and queue overruns also retire the offender.
Broadcast admission checks recipient capacity first; a flooding publisher cannot
cause a healthy subscriber to be retired for that publisher's overload.

The browser `disposable.policy` evidence covers malformed JSON/envelopes, unknown
capabilities, wrong arguments, unsafe/replayed IDs, extra routing fields, oversized
UTF-8 requests/commands/results, deeply nested and wide JSON, caught request floods,
resource limits, cross-instance unsubscribe and publish fanout overload. Parent-only
injection separately proves validation and cleanup do not rely on the Worker adapter.
Every rejection checks unchanged engine state and continued survivor events/calls.
Exact request/resource boundaries and the Worker-count ceiling also pass.
`bridge-policy.test.mjs` tests parser and UTF-8 boundary cases independently.

These limits bound admitted messages and tracked resources, not total RSS or a
request rate over time. Copying a guest string or dumping an evaluation result
still makes temporary host allocations before the size check; the guest module
cap remains relevant. Structured-clone overhead and browser queue bookkeeping are
not measured. The trusted Worker shell is part of the boundary; this does not
contain arbitrary code execution in that shell. Native fixture controls are described below; real engine authorization/transport
contracts remain unqualified.

### Native validation and Linux crash containment

Linux and Android JNI now run a native request validator before enqueueing guest
messages and again before host effects. It enforces UTF-8 request size (32 KiB),
JSON complexity (16 levels / 2,048 values), exact envelope shape, safe positive
request IDs, allowed operations/arguments, increasing IDs and subscription ownership.
The native adapter queue holds at most 32 requests; each generation may retain 16
subscriptions and 16 stalled calls. Source evaluation is limited to 64 KiB.
A validation/queue failure poisons the guest even if its script continues or catches
an exception. Host pumping rejects the poisoned generation, clears its queue and
retires its resources. The `late` fixture now drains only its own generation's calls.

Both native reports include 16 abuse cases, independent host-side invalid input,
resource cleanup with existing subscriptions/calls, and exact request/subscription/
stalled-call boundaries. The surviving instance continues receiving events and the
engine value remains unchanged in those rejection cases. Numeric writes now accept
finite floating-point values instead of relying on an integer conversion unwrap.
These are comparable fixture controls, not full browser/native scheduler parity:
native `delay` remains an immediate simulation, with no native timer queue; the
native harness does not implement the browser Worker-count or fanout admission policy.
The fixture still has trusted read/delivery/assertion helpers; it is not a hardened
production dispatcher. String conversion and result inspection can make transient
allocations, and aggregate process RSS is not bounded here.

The separate [Linux process report](../spikes/runtime/results/native-process.json)
tests a Rust supervisor with two runtime children. Child requests establish actual
fixture subscriptions and pending calls recorded in the parent. The harness then
injects an abort into one child and verifies SIGABRT termination, or injects a hang
and kills/reaps it after the 200 ms test deadline. The supervisor retires only that
child's generation; the other child evaluates code and receives events. A fresh child
restores baseline state and runs the complete native behavior and validation suites.
Core dumps are disabled for these deliberately crashing children only.

The experiment uses capped newline-framed IPC (64 KiB) and sequential commands,
with a bounded response channel. The fault controls are trusted CLI harness commands,
not guest engine capabilities. It proves containment of these injected failures,
not a fix for a known QuickJS crash, a security sandbox, or resource isolation against
arbitrary native code. Child processes still inherit OS permissions. The supervisor's
resource dispatcher handles only the lifecycle fixture; production IPC, authorization,
external effects, backpressure and durable engine ownership remain to be built.

### Android service processes and Binder recovery

The [Android process report](../spikes/runtime/results/android-process.json) now
records two non-exported bound services in distinct `:runtime_a` and `:runtime_b`
processes. Each service owns a persistent QuickJS instance on a dedicated
HandlerThread. The Activity process tracks each connection generation, its guest
subscription, stalled call, outstanding Binder commands and timeout callbacks.
Actual guest subscribe/stall requests are checked natively before the parent
records those resources; the IPC adapter uses fixed test commands, not a complete
engine protocol.

The emulator test injects a native `abort()` into service A, observes Binder death,
rejects the outstanding command and retires that generation's resources. It then
binds a replacement and verifies a fresh PID, baseline VM state and the complete
native behavior/validation suite. The second scenario blocks service A's native
thread, confirms service B still evaluates code and receives events during the
hang, then terminates A at a 1.5 s test watchdog deadline. Binder death and fresh
rebind recovery pass again. The test explicitly unbinds retired generations;
it does not depend on Android's automatic service reconnection behavior.

Late replies are rejected even when their request IDs collide with a currently
pending replacement command. Old-generation events are rejected as well. A dirty
VM does not survive process replacement; the survivor's subscription and pending
call do. All parent resource registrations, command promises, timeout callbacks
and service bindings are retired at the end. The final emulator process listing
showed the Activity process only, with both runtime service processes stopped.
The process report records UI heartbeat ticks separately from the original JNI
suite's heartbeat measurement.

This demonstrates containment of these injected faults on the recorded x86_64
Android emulator, not a known QuickJS crash fix or an arbitrary-native-code sandbox.
The services use separate processes **under the same app UID**, not
`isolatedProcess`; they retain app permissions. Native abort/hang controls are
trusted harness actions in non-exported test services, never guest capabilities.
The Activity still runs the original in-process JNI smoke suite separately; its
`catch_unwind` cannot contain aborts or segmentation faults. The service prototype
is the crash-containment candidate. Production Binder validation, byte/queue budgets,
engine integration, external-effect cancellation, background lifecycle policy and
broader device/OS coverage remain open.

Android testing uses an Android 16/API 36 x86_64 native Activity/JNI app, with a
main-thread heartbeat outside the runtime thread. Exact ticks/timing are recorded
in its report. The harness runs before device unlock and uses only non-secret
device-protected test reports. The renderer itself is not implemented.

### Physical ARM64 validation

On 2026-09-19, the same native fixtures and service-process tests passed on a
physical OnePlus CPH2493 reporting `arm64-v8a`, Android 16/API 36. The Rust report
identifies `aarch64`; APK inspection confirms that only the ARM64 native library
was packaged. [ARM64 input hashes](../spikes/runtime/results/inputs-arm64.json)
record the sources, APK and packaged library used for this run.

All 14 behavior checks passed over 20 cycles, as did the 16 native abuse tests,
resource boundaries, heap pressure and interruption checks. Both native abort and
hang scenarios observed Binder death, cleaned up parent resources, rejected stale
replies with colliding IDs and rebound fresh processes that passed the full native
suite. The UI and survivor runtime remained responsive. The final process listing
contained only the Activity process; both runtime services had stopped.

Current timings and heartbeat counts are stored in the reports, including the
execution-policy follow-up. These are single-run observations, not performance
guarantees. This verifies the previously missing
ARM64/physical-device case on one device; it does not establish coverage across
OEMs, Android versions or background lifecycle conditions.

### Dependency cycles and cancellation/recovery

The [execution-policy experiment](execution-policy-prototype.md) adds 13 shared
checks without changing the original 14-check report. Linux, Chromium, capped
browser Workers, Android x86_64 and physical ARM64 all pass. The candidate tracks
queue and dependency wait edges, rejects cycles before deadlock, and recovers the
queues after rejection. Cancellation skips queued work, propagates to child reads,
fences late guarded commits and retains a running operation's queue slot until it
settles. Previously dispatched engine effects remain visible. All tracked tasks
and queue tails are released after the tested operations settle.

The implementation lives in trusted fixture JS. This establishes supported runtime
semantics, not authoritative server scheduling, remote cancellation, durable
recovery or a final product error/cancellation contract. The linked proposal makes
those boundaries and remaining decisions explicit.

The candidate rquickjs and quickjs-emscripten packages declare MIT licenses;
full release dependency/license review remains separate.

Timings in reports are single-run diagnostics, not benchmarks or upper bounds.
No general memory-leak claim follows from 20 context cycles.

## Recommendation

Continue evaluating rquickjs/QuickJS-NG for the Rust server and a native Android
bridge. The shared-source and Promise bridge approach is viable in these tests.
Do not yet finalize the browser runtime or claim hardened script isolation.

Use a separate capped WASM module and disposable Worker per browser dashboard as
the candidate hosting strategy. Its full fixture suite and failure recovery now
pass in Chromium, including malformed-request validation and bridge budgets.
Linux child-process and Android service-process fault containment now have fixture
evidence, including Binder death, cleanup and explicit rebind on emulator and physical
ARM64 hardware. Dependency-cycle rejection and cooperative cancellation now have
shared fixture evidence. Next, review the candidate execution contract and qualify
host-enforced scheduling, remote action outcomes and production transport recovery. Preserve the uncapped
regression probes so upgrades cannot silently reintroduce reliance on the broken
aggregate runtime limit.

Before P0 closes, also qualify production-style host validation, external-effect
cancellation of external operations, dependency invalidation/propagation and
Android background lifecycle. The [experiment limits](../spikes/runtime/README.md#limits-of-the-evidence)
distinguish what is demonstrated from what still needs implementation.

The current conclusion is **behavioral feasibility with tested browser, Linux process and Android service containment**, not a final runtime selection or completion of the first milestone.
