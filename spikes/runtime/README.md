# P0 runtime experiment

An executable feasibility experiment, not the Talìa engine or a production
sandbox. The same `shared/bridge.js` and `shared/suite.js` execute in:

- Linux Rust/rquickjs (QuickJS-NG).
- Android Rust/rquickjs via a JNI entry point, on a background thread in a minimal
  native Activity. Java is used only for the test shell, not selected as the
  production client's implementation language.
- Browser QuickJS/WASM in a dedicated Worker, with a DOM heartbeat outside it.

## Reproduce

Requires Rust, Node/npm and a Playwright Chromium installation. Dependency
versions are recorded in Cargo.lock/package-lock.json. Run from repository root:

```sh
cargo run --manifest-path spikes/runtime/native/Cargo.toml --locked
# Linux only: deliberately aborts/kills isolated test children, not the supervisor.
cargo run --manifest-path spikes/runtime/native/Cargo.toml --locked -- --process-check
npm --prefix spikes/runtime ci
npm --prefix spikes/runtime run build
node spikes/runtime/run-browser.mjs
node spikes/runtime/compare-memory.mjs
node spikes/runtime/test-capped-memory.mjs
node --test spikes/runtime/bridge-policy.test.mjs
python3 spikes/runtime/check-results.py
```

Use `TALIA_CHROMIUM=/path/to/chromium` to override Playwright's browser. The browser
runner starts an ephemeral localhost server and closes it after testing. Its
JSON retains `qualified: false` for the deliberately uncapped regression candidate.
The runner exits zero when the capped disposable-Worker suite passes, exits 2 if
that suite does not report success, and fails on behavioral/harness errors. Full P0
qualification remains separate. `check-results.py` validates saved evidence; it
does not rerun the runtimes.

Android requires an x86_64 emulator, Rust's `x86_64-linux-android` target, NDK
27.0.12077973, libclang for bindgen, SDK 36, JDK 17+ and Gradle 8.13:

```sh
export TALIA_GRADLE=/path/to/gradle-8.13/bin/gradle
spikes/runtime/build-android.sh
adb -s <emulator> install -r spikes/runtime/android/app/build/outputs/apk/debug/app-debug.apk
adb -s <emulator> shell am start -W -n com.lelloman.talia.spike/.MainActivity
adb -s <emulator> logcat -d -v raw -s TaliaRuntimeSpike:I '*:S'
```

Wait for the Activity to display its report before collecting the dedicated log
tag. A fresh app process is needed to rerun; old log entries and existing report
files are not proof of a fresh run. Clear only this app's process/run as needed;
do not rely on wiping the device log. Reports also remain in device-protected
storage, but run-as cannot access them while the Android user is locked.
The test Activity is direct-boot aware and writes only non-secret fixture reports
to device-protected storage, allowing it to run on a locked test emulator. This
is a harness choice, not a production storage or lock-screen policy.
The APK and native library are build products, excluded from Git. Configure the
SDK with `ANDROID_SDK_ROOT`/`ANDROID_HOME` and override the NDK with `TALIA_NDK`.
The minimal APK requires API 26+, but only the recorded emulator was tested.

The Rust harness can also be run directly via ADB from `/data/local/tmp`; this
checks Android execution but does not replace the JNI/Activity run.

## What it exercises

Each host runs 20 fresh-context cycles over the identical source fixtures:

- Async JSON-only host messages, roundtrip values, rejection of unsupported
  transport values, and host errors.
- Per-instance queue serialization over a suspended Promise, independent instance
  progress, rejection recovery, and one cache load for concurrent getter calls.
- Event subscriptions, unsubscribe, explicit pending-call cancellation, and
  ignored duplicate/late replies within the context.
- Forced reload with active subscriptions and pending calls: host-owned generation
  cleanup, stale response/event rejection with colliding request IDs, and continued
  operation of an unrelated instance.
- Host writes, temporary function/state changes and baseline restoration in a
  new context, while the host's value survives reload.
- No ambient fetch/DOM/Node/renderer-definition globals in guest contexts.
- Interrupted infinite loops and heap-allocation pressure, with a working fresh
  context afterward. Browser/Android UI heartbeat is outside the JS host thread.

The bridge is a small trusted-fixture implementation, not the final permission
layer. Fixture engine operations are in-memory host simulations, not HTTP calls
to Talìa. Rust's queue-driven host pumps messages after JS suspends; the browser's
delay operation also uses a real host timer. This is genuine Promise/host bridging,
but not qualification of a production network scheduler.

The browser additionally runs `web/disposable-suite.js` in the parent page with
`dashboard-host.js` owning resources outside each `dashboard-worker.js`. Each
Worker gets a separate capped module. This tests the full shared suite over 20
fresh Workers, active-resource reload, four OOM workloads, an interrupted guest
loop and a harness-injected Worker hang. Another live Worker must continue making
progress, and a fresh Worker reruns the full suite after recovery. Fault commands
are trusted test controls, not guest engine capabilities or a production MCP API.

Native reports now include 16 abuse cases and request/resource boundary checks.
The native adapter validates before enqueueing and the host validates before
applying effects; violations poison the generation and host pumping retires its
resources. Native `delay` is still immediate, so timer budgets are not claimed.
The Linux `--process-check` command uses separate runtime children, bounded IPC,
parent-owned subscription/call records and injected abort/hang recovery. Its fault
commands are trusted harness controls and are not exposed to guest scripts.

## Limits of the evidence

- No server persistence, real monitoring, renderer compiler, MCP routing,
  authentication, or production engine is implemented.
- JNI is a coarse run-test entry point. Per-operation Kotlin/coroutine-to-JS
  bridging and a native dashboard renderer are not qualified.
- The serialization helper lives in trusted fixture JS; enforcement against
  malicious scripts bypassing that helper is not established.
- A missing global is not a security proof. Browser and native fixtures validate
  requests and bound their implemented queues/resources. Production authorization,
  complete native scheduling/backpressure and malicious native-code isolation remain open.
- Linux child-process tests contain an injected abort and hang, release parent-owned
  fixture resources and rerun the full native suite in replacements. Android JNI
  still runs in the app process; separate-service crash containment is untested.
- Cross-generation routing and forced disposal are tested with simulated host
  resources. Reconnect and cancellation of external effects remain unqualified.
- The cache test checks initial concurrency, not the full dependency/invalidation
  contract. Local state is in memory; no durable-state claim is made.
- Timeout interrupts and heap errors are observed, but hard process-level CPU/RSS
  limits are not established. Aggregate browser runtime limits fail across the three tested builds.
  A separately supplied capped WASM memory stops the pressure probes; it is a
  module-wide cap, not a guest-only budget or total browser RSS limit. The full
  shared suite now also runs in separate capped Workers, with OOM retirement,
  watchdog termination, parent-owned resource cleanup and fresh Worker recovery.
  Browser message/queue budgets and malformed-request validation now have abuse
  and exact-boundary tests. See the findings for limits and remaining constraints.
- Only Linux x86_64, Android emulator x86_64, and Chromium were exercised; ARM64,
  physical Android devices and other browsers remain unverified.

See [recorded findings](../../docs/runtime-prototype.md) and JSON results in
`results/`. These are observations for runtime selection, not performance SLAs.
