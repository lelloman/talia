# P0 runtime findings

Status: first experiment executed, 2026-09-18. **P0 qualification is incomplete.**
Runnable code and commands are in the [runtime experiment](../spikes/runtime/README.md).
The implementation plan's full runtime gate is not passed by these smoke tests.

## Candidates tested

| Host | Implementation | Environment |
|---|---|---|
| Linux | rquickjs 0.13.0 / QuickJS-NG, Rust CLI | x86_64 Linux |
| Android | Same Rust host cross-compiled, JNI-backed native Activity | x86_64 Android emulator; see recorded result |
| Browser | quickjs-emscripten 0.31.0 release-sync WASM inside Worker | Chromium 145.0.7632.6 |

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
updating its heartbeat. The ordinary-object pressure test raised an error under
the heap limit, **but a request to allocate 64 MiB in ArrayBuffers did not raise
an error with a 16 MiB limit**. The report therefore has `qualified: false`.
This experiment does not establish the root cause or actual resident memory of
the buffers; it establishes that the configured limit did not reject this workload.
It must not be treated as a hard total-memory budget.

The Android 16/API 36 app recorded 11 main-thread heartbeat ticks during its
114 ms native test run. The harness runs before device unlock and uses only
non-secret device-protected test reports. The renderer itself is not implemented.

The candidate rquickjs and quickjs-emscripten packages declare MIT licenses;
full release dependency/license review remains separate.

Timings in reports are single-run diagnostics, not benchmarks or upper bounds.
No general memory-leak claim follows from 20 context cycles.

## Recommendation

Continue evaluating rquickjs/QuickJS-NG for the Rust server and a native Android
bridge. The shared-source and Promise bridge approach is viable in these tests.
Do not yet finalize the browser runtime or claim hardened script isolation.

Resolve the browser memory-control gap next: reproduce it in a minimal upstream
fixture, evaluate a compatible newer/fixed build or alternate WASM distribution,
and rerun the same probes. Do not hide the failure by removing the pressure test
or merely blocking one constructor. If the embedding cannot enforce the desired
resource boundary, revise the hosting strategy explicitly.

Before P0 closes, also qualify production-style host validation, cancellation and
response generations, forced reload with active subscriptions, dependency-cycle
handling, native crash containment and ARM64 Android. The [experiment limits](../spikes/runtime/README.md#limits-of-the-evidence)
distinguish what is demonstrated from what still needs implementation.

The current conclusion is **behavioral feasibility with a browser qualification
blocker**, not a final runtime selection or completion of the first milestone.
