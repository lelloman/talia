# Talìa monitoring engine

Rust/Axum owns SQLite definitions, variables, computed state and action records.
Configurable JavaScript executes inside bounded QuickJS contexts. P3 adds Prometheus
and HTTP adapters, durable Pipelines, schedules and stateful Watches. This is a
loopback development service. Authenticated [MCP authoring](../docs/mcp-authoring.md),
[engine operations](../docs/mcp-engine.md) and [live control](../docs/mcp-live.md)
are available. [Configurable alerts](../docs/alerts.md) add staged policies, durable
SMTP/Telegram/FCM delivery and web/Android controls. See [qualification](../docs/alert-qualification.md)
for fixture and emulator evidence; live provider setup and production deployment
remain deployment work. See [monitoring](../docs/monitoring.md).

```sh
cargo test --manifest-path engine/Cargo.toml --offline
cargo build --manifest-path engine/Cargo.toml --offline
npm ci --prefix spikes/runtime
npm ci --prefix dashboard/web
bash dashboard/build-web.sh
TALIA_ENGINE_DB=/tmp/talia-development.db python3 dashboard/serve.py 18744
```

The development server starts and owns the Rust service; its database survives
shutdown. Open `http://127.0.0.1:18744`. Without `TALIA_ENGINE_DB`, it deliberately
uses the old P1 in-memory fixture. `TALIA_ENGINE_BIN` overrides the executable and
`TALIA_ENGINE_PORT` connects to an already-running server instead of starting one.

Android uses the same compiled UI and VM with native controls:

```sh
bash dashboard/build-android.sh
adb install -r dashboard/android/app/build/outputs/apk/debug/app-debug.apk
adb reverse tcp:18744 tcp:18744
adb shell am start -n com.lelloman.talia.dashboard/.MainActivity --ei port 18744 --ez durable true
```

The durable switch is explicit; P1 qualification continues exercising its frozen
protocol through the legacy mode. The default example grants access to `value`. The `monitoring` package declares
its named read/run grants and displays collected metrics, Watch flags and probe
results on both clients. Authenticated remote endpoints remain a later stage.

Connection indication is client-owned and survives dashboard script failure:
connecting..., disconnected while idle after failure, back online for three seconds
after completed recovery, otherwise hidden. It does not erase local VM state.
Reload/manual restart loads the saved baseline and discards temporary edits.
Special values remain tagged through the database, VM and rendering boundary;
`stateWire` in diagnostic reports preserves values that ordinary JSON cannot.

```sh
python3 engine/tests/emulator.py
python3 engine/qualify.py --emulator emulator-5570 --physical PHONE_SERIAL
```

Qualification records source/APK hashes and command results in `results/current.json`.
Omitting a device records it as missing and exits 2; a build is not device execution.
Tests remove their apps and reverse mappings. Stop only the disposable emulator
created for these tests (`adb -s emulator-5570 emu kill`).

See [values](../docs/engine-values.md), [storage](../docs/engine-storage.md),
[definitions](../docs/engine-definitions.md), [computed execution](../docs/engine-computed.md)
and [transport](../docs/engine-transport.md) for contracts and limits.

P4 now includes a trusted [authored catalog and package compiler](../docs/authoring-storage.md)
through the Rust Store API. Atomic source bundles reuse canonical engine records
and compile version-1 packages with the shared UI compiler. The
[agent authority layer](../docs/agent-authority.md) adds credentials, scoped permissions
and durable audits around trusted operations. [MCP authoring](../docs/mcp-authoring.md) now exposes these operations over stdio,
with operator credential setup and an authenticated private backend bridge. The
legacy `/engine` development HTTP route remains unauthenticated.

P4 also includes [client registration](../docs/client-registration.md): persistent
named clients, independent slots, fresh live IDs on replacement, authenticated host
reports and immediate availability checks for future live-command dispatch.

[Saved delivery](../docs/dashboard-delivery.md) now serves catalog packages and
revisioned slot assignments to both durable clients. `--seed` bootstraps authored
example sources once; generated package files are used only by legacy fixtures.
