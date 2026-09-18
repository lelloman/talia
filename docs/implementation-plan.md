# First implementation milestone

Status: implementation plan draft, 2026-09-18. **Web and native Android are both
required in the first milestone.** The product decisions in the
[specification](specification.md), [engine model](engine.md) and
[dashboard model](dashboards.md) remain authoritative. Proposed technical choices
below are evaluation inputs, not additional signed-off requirements.

The P0 experiments now include browser/native bridge abuse tests and Linux child-process crash containment; see [runtime findings](runtime-prototype.md).
P0 is not fully qualified and later phases have not started.

## Outcome

Deliver a local end-to-end system in which an MCP agent can define monitoring and
an interactive dashboard while Talìa stays running. A browser and a native Android
client load the same dashboard UI and JavaScript logic, receive server updates,
and invoke engine actions. Monitoring survives client disconnection and server
restart with its persisted state intact.

This milestone establishes the foundation. Production homelab cutover, complete
Grafana parity, and full Simple Agents/Crumbles integrations are later deliveries,
not removed from the product scope. The first delegated action is a local probe
Pipeline; adapters for sessions and tickets remain explicit follow-on work.

## Demonstration scenario

1. Start an isolated Prometheus fixture with controllable host metrics and an HTTP
   endpoint that returns a per-service disk-usage breakdown. Do not depend on or
   modify production homelab for deterministic tests.
2. Through MCP, create the DataSources, collection Pipeline and typed Variables
   for CPU, memory and available disk space, including quality and timestamps.
3. Add a computed value with declared dependencies and a getter/setter using
   internal state and injected time. Demonstrate cache expiry and invalidation.
4. Create one reusable disk-pressure Watch definition and two instances with
   different inputs/thresholds and independent persisted flags. A staged low-space
   condition triggers the breakdown probe Pipeline and publishes its result.
5. Author a dashboard through MCP containing a persistent navigation screen and
   selectable content screens. Include a metric chart, status/text, a slider,
   a switch and an action button. The slider affects the displayed time window,
   the switch controls an authorized monitoring setting, and the button invokes
   the same probe Pipeline explicitly.
6. Load the exact same saved UI and ViewModel artifacts on web and native Android.
   Also demonstrate per-client assignment using an alternate composition through
   the same language. Shared definitions and instance parameters remain distinct.
7. Subscribe from both clients. Disconnect them and verify monitoring continues;
   reconnect and show current values with accurate age/quality.
8. Target one live client through MCP, modify its ViewModel and observe its dirty
   state. The other instance and saved definition are unchanged. Reject live View
   definition edits. Reload clears the temporary modifications, while a server
   action already performed remains in effect.
9. Update a shared Watch definition and verify both references adopt it according
   to the chosen activation policy. Changing only one instance's parameters must
   not alter the other. Exercise equivalent shared UI/function references.
10. Restart the server and verify definitions, retained values, computed internal
    state and Watch flags recover. Values keep their original timestamps; restart
    must not masquerade as a fresh observation or blindly repeat completed actions.

## Proposed sequence

Each step has a concrete exit condition. Client-facing capabilities are developed
and tested on both platforms in the same step; Android is not a final port.

| Step | Deliverables | Exit condition |
|---|---|---|
| P0: Runtime feasibility | Server, Android and browser runtime harnesses; shared JS fixtures; decision record | Same supported semantics and bounded failure behavior on all three hosts |
| P1: Contracts and UI feasibility | Versioned value/operation envelopes, restricted UI grammar and typed tree, both minimal renderers | One UI/VM fixture works unchanged on web and Android; malformed definitions fail clearly |
| P2: Durable engine | Rust/Axum service, storage adapter, definitions, Variables/computed state, per-instance execution queues, subscriptions | Concurrent callers serialize correctly; restart and invalidation tests pass |
| P3: Collection and Watches | Prometheus and HTTP adapters, automatic/explicit Pipelines, persisted Watch instances and actions | Staged and skipped-threshold scenarios have documented outcomes; both clients show results |
| P4: MCP and live instances | Persistent authoring tools, validation, client registration/targeting, temporary VM execution and reload | Authoring and live capabilities stay separate; effects and client identities are traceable |
| P5: Complete demonstration | Full scenario, repeatable startup, web build, Android debug APK, recovery instructions | Acceptance matrix passes with evidence from both clients and server |

Minimal MCP tools can be introduced alongside P2/P3; P4 completes their lifecycle
and isolation semantics. Each phase should leave a runnable increment. Do not
build a full component library or all integration adapters before completing
this slice.

## Prototype P0: JavaScript hosts and bridge

Evaluate a QuickJS-family implementation first; this is a candidate family, not
an assumption of identical versions, feature sets or bytecode formats. Ship
portable source/definitions initially, not cross-runtime bytecode.

| Host | First candidate | Qualification work |
|---|---|---|
| Rust server | rquickjs | Async Rust/JS calls, instance queues, deadline interruption, heap limits, lifecycle and worker isolation |
| Android | QuickJS binding; evaluate Zipline's low-level suitability or a thin native bridge | Arbitrary authored JS without requiring Kotlin/JS compilation, Promise bridging, interruption, supported ABIs and lifecycle cleanup |
| Web | QuickJS compiled to WebAssembly inside a dedicated Worker | Async bridge, script globals/capabilities, memory behavior, worker termination and reload; compare native Worker JS if needed |

Source basis checked 2026-09-18:

- [rquickjs](https://github.com/DelSkayn/rquickjs) provides Rust bindings to
  QuickJS-NG with async integration. Its
  [AsyncRuntime API](https://docs.rs/rquickjs/latest/rquickjs/struct.AsyncRuntime.html)
  exposes interrupt and memory controls. These are candidate controls to test,
  not proof of Talìa's execution guarantees.
- [Zipline](https://github.com/cashapp/zipline) embeds QuickJS and is oriented
  toward Kotlin/JS modules. Its documented deployment path must not be mistaken
  for a ready-made arbitrary-script runtime meeting Talìa's needs.
- [quickjs-emscripten](https://github.com/justjake/quickjs-emscripten) supplies a
  WebAssembly embedding candidate for browser execution.
- [Worker termination](https://developer.mozilla.org/en-US/docs/Web/API/Worker/terminate)
  stops a browser worker immediately; it does not run graceful cleanup. Host-side
  subscription disposal and old-generation reply rejection now have simulated-host
  tests; production transport cancellation and reconnect remain open.

A browser Worker alone is not a capability sandbox: browser-native network and
other APIs must not become an unreviewed route around the engine bridge. Likewise,
a JS context alone is not assumed to contain native-runtime crashes. Evaluate
server worker/process containment and Android recovery rather than claiming hard
isolation from a library feature list.

Run one shared fixture suite against each candidate:

- Promises, async handlers, scalar/structured values, explicit errors and clock
  injection. Fix a portable value representation; reject unsupported values.
- A suspended getter with concurrent reads/setters: no interleaving on that
  instance, while another instance still progresses.
- Host read/write/subscribe calls, cancellation, unsubscription, duplicate and
  late replies, and reconnect/reload generation changes.
- Infinite loops, allocation pressure, rejected Promises and stalled host calls:
  bounded failure without freezing the renderer or unrelated server work.
- Repeated load/dispose cycles, live function replacement and state mutation;
  fresh reload restores baseline code/state and releases subscriptions.
- Unauthorized host/network/file access is unavailable through the exposed API.
  Verify that injected live code cannot reach the renderer's View definition.

Record exact versions, supported platforms/ABIs, license, build reproducibility,
startup/bridge latency, memory use, termination behavior and cleanup evidence.
Measure on an Android device/emulator and actual browsers, not only a desktop
JavaScript runner. Document any missing environment as unverified. Select the
runtime only after this evidence exists. If no candidate meets the boundary,
revise the hosting/isolation approach before implementation proceeds.

## Prototype P1: Shared UI and live runtime

Propose a minimal grammar with named components, typed literal properties, simple
state bindings, named actions and explicit conditional/repeated content. Parse
restricted JSX-like source to a validated versioned tree; do not execute arbitrary
JSX expressions as application code. Evaluate an existing parser versus a small
parser before selecting one. Produce useful source-location errors.

A Kotlin/Compose Android renderer and a DOM-based web renderer are candidate
implementations. The public contract is the shared tree and behavior, not either
framework. Use native Android controls; an Android WebView rendering the web app
does not demonstrate the agreed native client.

Initial vocabulary proposal: row/column, grid, scroll container, text/status,
button, slider, switch, line chart and navigation control. Cover screen composition,
responsive sizing, bindings, a list and conditional content with a small fixture.
This is milestone coverage, not a final language/component catalog.

Compare observed states and actions after identical inputs, not pixel equality.
Check narrow/wide layouts, long text, semantic labels, focus/touch interaction and
observable errors. Reuse the existing [brand assets](branding.md) without changing
the selected identity.

## Prototype P2: Persistence, serialization and activation

Evaluate SQLite as the initial storage candidate using the real execution path.
Persist versioned definitions, instance parameters, values/quality, configured
history, computed/Watch state and action records. Show consistent restart recovery
and a backup/restore procedure; do not serialize live JS heaps or credentials into
script state. Define the supported state value format explicitly.

Use per-instance execution queues covering asynchronous evaluation. Do not keep
a global DB write transaction open while a getter awaits network IO. Probe cycles,
recursive reads, concurrent invalidation, timeout, server termination and late
completion before fixing the locking and commit design.

Record decisions on these questions as part of the prototype, using concrete
failure traces rather than leaving implementation to guess:

- Whether failed evaluation commits or discards mutations to internal state.
- When value/history/state and action intent become durable relative to responses.
- How invalidation reaches an arbitrary user-managed cache without overriding its
  private fields, and how subscribed results are refreshed/coalesced.
- How shared-definition updates activate across references, drain/cancel in-flight
  work and migrate incompatible state without a service restart.
- How dependency-cycle rejection, reconnect snapshots and subscription event order
  prevent stale or recursive work from becoming invisible failures.

Successful serialization tests do not establish rollback, multi-variable
transactions or exactly-once external effects. Keep those claims separate.

## Acceptance matrix

| Area | Required evidence |
|---|---|
| Shared clients | Same UI/VM artifact IDs in browser and Android; matching actions and state transitions |
| Native Android | Runnable debug APK and instrumentation/device evidence, not just shared-code unit tests |
| Runtime edits | Create/change definitions while service stays up; invalid change leaves active version usable |
| Reuse | Shared definition change reaches references; instance parameters and mutable state stay independent |
| Computed values | Cache/clock/state, setter, dependencies, refresh and invalidation with concurrent readers |
| Atomicity | One instance does not interleave across awaits; independent instances continue |
| Collection | Automatic, Watch-triggered and explicit Pipeline runs; failed sources produce quality/errors |
| Watches | Durable flags, re-arming policy, skipped thresholds, and action recovery tested |
| MCP | Persistent edits, direct engine operations and targeted live VM actions follow distinct permissions |
| Reload | Dirty VM resets; no View rewrite via live code; subscriptions clean up; server effects persist |
| Recovery | Restart retains definitions/state and preserves measurement age; clients reconnect explicitly |
| Failure isolation | Misbehaving JS can be stopped without freezing unrelated work or UI |

Use deterministic clocks and source fixtures for semantics, plus real HTTP
Prometheus integration and real client runtimes. Include a small cross-platform
conformance suite in routine checks. Do not declare Android support from server
or browser tests alone.

## Proposed code organization

After the prototypes establish the contracts, a starting layout is:

```text
crates/       Rust engine, API, persistence, JS host and MCP integration
contracts/    Versioned shared schemas and cross-platform fixtures
runtime/      Shared JS support code and conformance fixtures
clients/web/  Web renderer, client bridge and application shell
clients/android/  Native renderer, JS host and client bridge
examples/     Shared dashboard/monitoring definitions for the milestone
spikes/       Bounded runtime/compiler/recovery experiments with findings
```

This is a proposal, not a requirement to make each directory an independent
service. Core logic belongs on the server; clients must not duplicate collection
or Watch execution. MCP is an adapter to engine/authoring operations plus targeted
live interaction, not a second implementation of those operations.

## Subsequent work and production boundary

After this milestone, implement Simple Agents lifecycle/result integration and
Crumbles ticket lifecycle/outcome triggers, then notification channels and richer
alert management. Production rollout also needs identity/permissions, operational
limits, secret provisioning and the [migration work](migration.md).

The first milestone must still enforce its explicit local-development access
boundary and script capabilities. It is not an unauthenticated production service.
It does not deploy to homelab or retire existing monitoring. Keep those actions
separate from building and testing the foundation.
