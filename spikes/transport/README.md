# Client–server transport experiment

A loopback Rust/axum server and one shared JavaScript client adapter/suite, run in
Linux QuickJS, native Android QuickJS and capped browser QuickJS/WASM. This is a
transport experiment, not the production engine, renderer or public API.

The server maintains a numeric value, revision and action ledger in memory. Two
logical clients share each test namespace. HTTP runs outside authored JavaScript:
Rust network threads on Linux/Android and host Worker fetches on the web. Native
Android uses the existing JNI embedding in a separate test Activity; no WebView.
The UI heartbeat remains outside the JavaScript/network threads.

## Run

From repository root, with the runtime experiment's Rust, npm, Chromium and Android
prerequisites installed:

```sh
cargo build --manifest-path spikes/transport/server/Cargo.toml --locked
npm --prefix spikes/runtime ci
npm --prefix spikes/runtime run build
spikes/transport/build-web.sh
# Keep this running; binds only 127.0.0.1. Port 0 chooses and prints an available port.
cargo run --manifest-path spikes/transport/server/Cargo.toml --locked -- 18743
```

In another shell:

```sh
cargo run --manifest-path spikes/runtime/native/Cargo.toml --locked -- --transport 18743 > spikes/transport/results/linux.json
node spikes/transport/run-browser.mjs 18743 > spikes/transport/results/browser.json
python3 spikes/transport/test-server.py 18743 > spikes/transport/results/server.json
```

Open `http://127.0.0.1:18743/` to run the browser fixture with a visible heartbeat.
For Android, build the requested ABI using the existing script, then use the
collector (which installs, sets an ADB reverse mapping, tests, removes the mapping,
force-stops and uninstalls the test package in a finally block):

```sh
TALIA_ANDROID_ABI=arm64-v8a TALIA_GRADLE=/path/to/gradle-8.13/bin/gradle spikes/runtime/build-android.sh
python3 spikes/transport/run-android.py DEVICE_SERIAL 18743 --name android-arm64 --runtime-regression
# Build x86_64 for an emulator, then use --name android instead.
# After collecting all four matching reports:
python3 spikes/transport/record-inputs.py
python3 spikes/transport/check-results.py
```

`--runtime-regression` also runs the original 14 behavior/27 execution checks and
Android process-recovery harness and updates their reports. Evidence validators
check saved results and source hashes; they do not rerun tests. APKs/build products
are ignored. Test device serials are not stored in reports.

## Candidate contract

- Request envelope: `{channel, epoch, id, op, args}`; the HTTP host adds the test
  namespace. The host chooses the endpoint. Replies must match the originating
  route. Each client tracks pending IDs within a generation; reconnect increments
  that generation and rejects old replies even when numeric IDs collide.
- `read` returns `{value, revision}`. Reads and writes proceed during another
  request's I/O. Older reads fail rather than replace newer observed state.
- `watch` is bounded HTTP long polling with a revision cursor and latest-snapshot
  delivery. It coalesces intermediate values; this is not an event history or
  lossless alarm feed. Reconnect obtains a fresh snapshot before resuming retained
  listeners. There is no automatic reconnect/backoff policy in this fixture.
- Mutations have caller-supplied action IDs. A ledger tracks `accepted`,
  `completed`, `cancelled` or `failed`; an absent record yields `unknown`. Concurrent
  or explicit retries of the same ID/arguments share one effect. Conflicting
  arguments are rejected. The adapter never automatically resubmits an action.
- Losing an action response rejects locally with `outcome unknown` and its action
  ID. Reconnect/status lookup can recover its server outcome. A missing record is
  not evidence that no effect occurred. Losing a connection does not cancel work
  already accepted by the server.
- Cancellation before local dispatch skips the request. After dispatch, local
  cancellation and server cancellation are distinct: the local Promise rejects
  immediately, while the cancellation RPC reports whether the server cancelled
  accepted work or had already completed it. Completed effects are not undone.
- Successful actions remain successful even if their delayed response contains a
  snapshot older than the client's current view. Only presentation of that stale
  snapshot is suppressed.

These are evaluated policies, **not signed-off API defaults**. `disconnect` tests
adapter-generation replacement, not a renderer reload. Existing runtime tests
separately cover actual Worker/context/process replacement.

## Failure injection and bounds

The `test` capability controls named gates, admission observations and reply loss.
These controls are trusted harness operations, not production capabilities. To
simulate lost delivery, the server commits an action, sends response headers and a
partial JSON body, then fails the body stream. This exercises actual HTTP failure
in both adapters without relying on a fake network error or browser pre-header
connection recovery. Server RPC tasks continue independently of HTTP handler loss.

Server mutation/ledger updates use short synchronous mutex sections, with no guard
held across I/O. Test gates expire after five seconds; a pre-effect expiry records
a failed action. Expiry of a post-effect reply delay cannot change its outcome.
Server bounds include 32 KiB request bodies, 64 executing requests, 32 namespaces,
128 action records per namespace and bounded fault-control records. Client hosts
limit concurrent requests to 32 and the suite to 2,000 dispatched requests.
Long polls and network timeouts bound outstanding work after disconnect; the
fixture does not immediately abort physical I/O. Reports require it to drain.

The native HTTP adapter is intentionally narrow: loopback HTTP/1.1, fixed endpoint,
JSON with Content-Length and no redirects/TLS/pooling. Browser networking uses
fetch; guest QuickJS has no fetch or platform objects. The trusted shared client
is not a malicious-script authorization boundary. This server has no authentication,
TLS, durable ledger, retention/eviction, server-incarnation negotiation or automatic
server-restart recovery. Restart loses values and action records; no exactly-once
external-effect or durable-recovery guarantee follows from these checks. It must
remain a local test harness. Automatic dependency propagation, production transport
selection and Android background lifecycle remain open.

## Evidence

On 2026-09-19, all four hosts passed the same 25 transport checks; the server passed
10 additional HTTP validation, concurrency and failure-recording checks. The browser
uses a capped 16 MiB WASM module; Android includes emulator x86_64 and physical ARM64.
Reports, APK identities and input hashes are in `results/`. The original runtime
checks were also rerun on the changed native embeddings. Test apps were uninstalled
and the temporary emulator/server were stopped after collection.

The [transport findings](../../docs/transport-prototype.md) describe what this
qualifies and the next implementation step.
