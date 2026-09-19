# P0 runtime experiment

An executable feasibility experiment, not the Talìa engine or a production
sandbox. **Its execution fixtures still implement the superseded whole-operation
serialization model.** The agreed contract now allows same-instance interleaving
across `await`, short atomic updates and configurable read sharing. The fixtures
and recorded results need revision; passing them does not qualify the new contract. The same `shared/bridge.js` and `shared/suite.js` execute in:

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

Android requires an x86_64 emulator or ARM64 device, the corresponding Rust
`x86_64-linux-android` or `aarch64-linux-android` target, NDK
27.0.12077973, libclang for bindgen, SDK 36, JDK 17+ and Gradle 8.13:

```sh
export TALIA_GRADLE=/path/to/gradle-8.13/bin/gradle
# Defaults to x86_64; use arm64-v8a for an ARM64 device.
TALIA_ANDROID_ABI=arm64-v8a spikes/runtime/build-android.sh
adb -s <emulator> install -r spikes/runtime/android/app/build/outputs/apk/debug/app-debug.apk
adb -s <emulator> shell am start -W -n com.lelloman.talia.spike/.MainActivity
adb -s <emulator> logcat -d -v raw -s TaliaRuntimeSpike:I '*:S'
```

Wait for the Activity to display its report before collecting the dedicated log
tag. In addition to the original native JSON and `UI_TICKS`, the log now includes
`PROCESS_RESULT=` with the Android service-process report. The app deliberately
aborts one runtime service and watchdog-kills a hung service; the Activity and a
second runtime service must stay responsive. Replacements rerun the native suite.
Faults are confined to non-exported test services in dedicated app processes.
The process report is also saved as `android-process.json` in device-protected
files. Keep the latest complete run together when recording results. A fresh app process is needed to rerun; old log entries and existing report
files are not proof of a fresh run. Clear only this app's process/run as needed;
do not rely on wiping the device log. Reports also remain in device-protected
storage, but run-as cannot access them while the Android user is locked.
The test Activity is direct-boot aware and writes only non-secret fixture reports
to device-protected storage, allowing it to run on a locked test emulator. This
is a harness choice, not a production storage or lock-screen policy.
The APK and native library are build products, excluded from Git. Configure the
SDK with `ANDROID_SDK_ROOT`/`ANDROID_HOME` and override the NDK with `TALIA_NDK`.
The minimal APK requires API 26+. Android 16/API 36 has been tested on the
recorded x86_64 emulator and a physical ARM64 OnePlus CPH2493. The build script
selects and packages only the requested ABI, even when other local JNI builds exist.
Physical-device results use `android-arm64.json` and `android-process-arm64.json`;
`inputs-arm64.json` records source and APK/library hashes separately from the
earlier emulator evidence.

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

The shared suite also reports 13 `execution` checks using the candidate
`ExecutionScheduler`: wait-cycle rejection, cancellation propagation, queue
recovery, guarded commits and preserved dispatched effects. See the
[execution-policy proposal](../../docs/execution-policy-prototype.md) for the tested
semantics and limits. These results are separate from the original 14 checks and
describe the superseded policy, not acceptance of the current async contract.

## Limits of the evidence

- No server persistence, real monitoring, renderer compiler, MCP routing,
  authentication, or production engine is implemented.
- JNI uses coarse fixture entry points and fixed service test actions. Production
  Kotlin/coroutine bridging, Binder validation/budgets and a native dashboard renderer
  are not qualified.
- The serialization helper lives in trusted fixture JS; enforcement against
  malicious scripts bypassing that helper is not established.
- A missing global is not a security proof. Browser and native fixtures validate
  requests and bound their implemented queues/resources. Production authorization,
  complete native scheduling/backpressure and malicious native-code isolation remain open.
- Linux child-process tests contain an injected abort and hang, release parent-owned
  fixture resources and rerun the full native suite in replacements. Android bound
  services now pass equivalent injected faults, Binder death, generation cleanup
  and fresh-process rebind tests. These processes share the app UID; this is not
  an OS permission sandbox. The original JNI smoke test still runs in the Activity
  process separately.
- Cross-generation routing and forced disposal are tested with simulated host
  resources. Android service rebind is tested; server/network reconnect and
  cancellation of external effects remain unqualified.
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
- Linux x86_64, Android emulator x86_64, one physical ARM64 Android 16 device,
  and Chromium were exercised. Other devices, OS versions and browsers remain
  unverified.

See [recorded findings](../../docs/runtime-prototype.md) and JSON results in
`results/`. These are observations for runtime selection, not performance SLAs.
