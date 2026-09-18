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
npm --prefix spikes/runtime ci
npm --prefix spikes/runtime run build
node spikes/runtime/run-browser.mjs
```

Use `TALIA_CHROMIUM=/path/to/chromium` to override Playwright's browser. The browser
runner starts an ephemeral localhost server and closes it after testing. Its
JSON reports `qualified: false` and exits 2 for a failed memory test; behavioral
failures also exit nonzero. Read the full result to distinguish them.

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

## Limits of the evidence

- No server persistence, real monitoring, renderer compiler, MCP routing,
  authentication, or production engine is implemented.
- JNI is a coarse run-test entry point. Per-operation Kotlin/coroutine-to-JS
  bridging and a native dashboard renderer are not qualified.
- The serialization helper lives in trusted fixture JS; enforcement against
  malicious scripts bypassing that helper is not established.
- A missing global is not a security proof. Host payload validation, authorization,
  worker/process containment and hostile native-runtime crash behavior remain open.
- Late responses are tested within a context. Cross-generation response routing,
  reconnect, cancelled external effects and forced disposal with active
  subscriptions are not yet qualified.
- The cache test checks initial concurrency, not the full dependency/invalidation
  contract. Local state is in memory; no durable-state claim is made.
- Timeout interrupts and heap errors are observed, but hard process-level CPU/RSS
  limits are not established. Browser ArrayBuffer limits fail in this candidate.
- Only Linux x86_64, Android emulator x86_64, and Chromium were exercised; ARM64,
  physical Android devices and other browsers remain unverified.

See [recorded findings](../../docs/runtime-prototype.md) and JSON results in
`results/`. These are observations for runtime selection, not performance SLAs.
