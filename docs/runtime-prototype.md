# P0 runtime findings

Status: browser bridge validation and budgets tested, 2026-09-18. **P0 qualification is incomplete.**
Runnable code and commands are in the [runtime experiment](../spikes/runtime/README.md).
The implementation plan's full runtime gate is not passed by these smoke tests.

## Candidates tested

| Host | Implementation | Environment |
|---|---|---|
| Linux | rquickjs 0.13.0 / QuickJS-NG, Rust CLI | x86_64 Linux |
| Android | Same Rust host cross-compiled, JNI-backed native Activity | x86_64 Android emulator; see recorded result |
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
- [Android APK result](../spikes/runtime/results/android.json)

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
contain arbitrary code execution in that shell. Native hosts have not yet adopted
this policy, and real engine authorization/transport contracts remain unqualified.

Android testing uses an Android 16/API 36 x86_64 native Activity/JNI app, with a
main-thread heartbeat outside the runtime thread. Exact ticks/timing are recorded
in its report. The harness runs before device unlock and uses only non-secret
device-protected test reports. The renderer itself is not implemented.

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
Next, investigate native-runtime crash containment and apply comparable host
validation/resource controls to the native bridge. Preserve the uncapped
regression probes so upgrades cannot silently reintroduce reliance on the broken
aggregate runtime limit.

Before P0 closes, also qualify production-style host validation, external-effect
cancellation, dependency-cycle handling, native crash containment and ARM64
Android. The [experiment limits](../spikes/runtime/README.md#limits-of-the-evidence)
distinguish what is demonstrated from what still needs implementation.

The current conclusion is **behavioral feasibility with tested browser module containment**, not a final runtime selection or completion of the first milestone.
