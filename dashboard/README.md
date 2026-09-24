# Shared dashboard implementation

P1 renders the same restricted UI and JavaScript ViewModel on the web and native
Android. It uses the existing loopback Rust engine fixture. This is a development
client, not the production monitoring service. Durable engine storage and
[MCP authoring](../docs/mcp-authoring.md) and [live control](../docs/mcp-live.md) are now available in durable mode;
the production web host uses LelloAuth OIDC and the homelab deployment described
in [deployment documentation](../deploy/README.md). Its application chrome adopts
[LelloDesign](../docs/lellodesign-adoption.md).

## Build and run

From the repository root, using the locked dependencies already used by P0:

```sh
npm ci --prefix spikes/runtime
npm ci --prefix dashboard/web
cargo build --manifest-path spikes/transport/server/Cargo.toml --locked
bash dashboard/build-web.sh
python3 dashboard/serve.py 18744
```

Open `http://127.0.0.1:18744`. `serve.py` binds only to loopback and owns its Rust
fixture child process. Ctrl-C stops both. It serves repository assets for local
development; do not expose it to a network.

```sh
bash dashboard/build-android.sh                    # x86_64
TALIA_ANDROID_ABI=arm64-v8a bash dashboard/build-android.sh
```

The Android build uses the existing SDK, NDK 27.0.12077973 and Gradle 8.13 cache.
`TALIA_GRADLE`, `TALIA_NDK` and `ANDROID_HOME` can select those installations.
The APK is `dashboard/android/app/build/outputs/apk/debug/app-debug.apk`.
For a manual device session, reverse the development port with ADB and launch
`com.lelloman.talia.dashboard/.MainActivity` with integer `port` extra. Force-stop,
uninstall the test app and remove that reverse mapping after the session.

## Authoring and clients

[Contract v1](contracts/v1/README.md) defines the source language and envelopes.
[monitor.package.json](examples/monitor.package.json) references the example UI,
ViewModel and shared notice definition. `node dashboard/compile.mjs` compiles a
coherent saved package. Both builds call it. Change source and recompile to make
Update available appear; running clients adopt it only through Reload.

Each client retains its own saved-dashboard ID. The web host exposes
`talia.selectDashboard(id)` for client configuration; Android accepts the
`dashboard` launch extra and persists it alongside its engine port. The development
server serves `dashboard/generated/ID.json` for the selected ID, while all IDs use
the same engine. Cached baselines are scoped by dashboard ID. The default is
`monitor` in this legacy fixture mode. Durable mode instead uses
[server-managed assignments](../docs/dashboard-delivery.md) and supports
[authenticated MCP configuration](../docs/mcp-authoring.md).

The web client's display settings persist its manual dp scale (0.25–8 CSS px per
dp) and navigation placement. Android uses native density and keeps its navigation
preference locally. The ViewModel continues across screen/composition changes.
Backgrounding pauses it; resume refreshes subscriptions and reconciles writes.
Internal errors show diagnostics and require Restart. Reload/Restart discards
temporary VM edits and preserves server effects.

`shared/ui.js` runs only in trusted host/UI contexts. `shared/vm.js` and dashboard
scripts run in a capped QuickJS runtime. Android resolves UI in a separate native
QuickJS context and creates actual Android Views; it never uses a WebView. Browser
scripts use a disposable WASM Worker. Host-issued IDs, resource checks, queue
bounds and lifecycle epochs guard engine dispatch. Scripts have no direct I/O.

## Verification

```sh
node --test --test-reporter=tap dashboard/tests/compiler.test.mjs
node --test --test-reporter=tap dashboard/tests/vm.test.mjs
cargo test --manifest-path dashboard/native/Cargo.toml --locked
python3 dashboard/qualify.py --emulator emulator-5570 --physical PHONE_SERIAL --p0-regression
```

The qualification runner builds both Android ABIs, checks exact packaged shared
artifacts, exercises native controls and real Chromium visibility, compares final
ViewModel state, records source/APK hashes, and verifies app cleanup. Omitting
`--physical` still builds ARM64 but explicitly produces an incomplete report and
exit code 2. It never substitutes an emulator or older report for a physical run.
The optional P0 regression run writes a new report under `dashboard/results`;
historical P0 evidence stays untouched.

[Qualification status](../docs/p1-qualification.md) records the current evidence
and limits. Test ports, debug live-script intents and host inspection objects are
development facilities. They are not the authenticated production MCP interface.

For persistent server state and connection recovery, run the [P2 durable engine](../engine/README.md). Set `TALIA_ENGINE_DB` for the web development host and pass `--ez durable true` to Android. Without those switches these P1 commands retain the in-memory fixture.

P3 adds the shared `monitoring` package, rendered by the same DOM/native hosts.
It displays CPU, memory, two disks, Watch flags, data age/quality, investigation
results and a manual investigation action. Build scripts compile both example
packages. On web select it with `talia.selectDashboard('monitoring')`; on Android
launch with `--ez durable true --es dashboard monitoring` against a configured
engine. `engine/tests/monitoring_fixture.py` supplies isolated development fixtures.

Package `grants` lists `reads`, `writes` and `runs`; absent grants retain the P1/P2
`value` read/write capability. Both hosts enforce grants independently of guest JS.
ViewModels use `ctx.read(id)`, `ctx.subscribe(id,handler)`, `ctx.write(id,value)` and
`ctx.run(pipelineId)`; single-argument `ctx.write(value)` remains supported.
Run admission returns a receipt; named `monitor.<id>` resources expose live status.
Backgrounding detaches client subscriptions while autonomous server collection
continues. Reconnect restores each resource and reconciles action identities before
showing back online. Dashboard reload restores the saved UI/VM baseline.

The durable mode now uses [persistent client registration](../docs/client-registration.md).
Browser tabs share registration but keep separate slots and selections; Android
preserves registration across process recreation. Reload replaces the live instance
while network reconnect preserves it. Registry credentials remain in the host.

Durable clients now use [saved dashboard delivery](../docs/dashboard-delivery.md).
Updates leave the loaded runtime intact until explicit reload; cached baselines
include per-slot parameters and loaded assignment revision. Generated package files
remain available for the legacy fixture mode.

## Fullscreen monitoring (web)

Open **Dashboard → Fullscreen** for a monitoring surface without the application
shell. Configure an ordered playlist in **Settings → Monitoring display**, set
seconds per dashboard, and optionally enable automatic rotation on entry. Use
Previous/Next, Play/Pause and Exit. Left/Right arrows and Page Up/Page Down cycle
through dashboards while monitoring is active. Move the pointer, touch the surface or use the
keyboard to reveal controls. Escape exits. Configuration persists for this browser
and account; other devices have independent settings.

Entering fullscreen preserves the current live dashboard. Cycling pauses inactive
ViewModels and reuses up to four matching instances per tab, retaining their temporary
state. Dirty agent edits and dashboard errors pause rotation. Hidden tabs and
disconnections suspend timing. Browser fullscreen refusal falls back to an
in-window monitoring surface. Native Android and individual Screen cycling are
not part of this increment.

Qualification:

```sh
node dashboard/tests/monitoring-mode.test.mjs
node dashboard/tests/monitoring-mode.mjs
node dashboard/tests/access.mjs
```

The first checks scheduler edge cases. The second uses real Chromium fullscreen
and a fixture dashboard selector for settings, rotation, fallback, error/dirty
handling, authorization catalog changes and responsive layout. The third covers
the real HTTPS/OIDC server and verifies fullscreen entry/exit retains the live
instance, alongside existing sharing, revocation, MCP and session checks.
