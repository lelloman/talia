# P0 runtime findings

Status: memory comparison and reload follow-up executed, 2026-09-18. **P0 qualification is incomplete.**
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
Worker overhead or total RSS. The full behavioral suite still runs on the normal
module; the capped module currently runs the pressure probes only. Recovery and
budget handling need integration before adopting this as the browser host.

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

Next, integrate the capped-module strategy into a disposable Worker per dashboard
instance, run the full bridge/lifecycle suite under that cap, and verify that OOM
or Worker termination retires host resources while another dashboard continues.
Bound host-side queues and transport payloads separately. Preserve the uncapped
regression probes so package upgrades cannot silently reintroduce reliance on the
broken aggregate runtime limit.

Before P0 closes, also qualify production-style host validation, external-effect
cancellation, dependency-cycle handling, native crash containment and ARM64
Android. The [experiment limits](../spikes/runtime/README.md#limits-of-the-evidence)
distinguish what is demonstrated from what still needs implementation.

The current conclusion is **behavioral feasibility with a browser qualification
blocker**, not a final runtime selection or completion of the first milestone.
