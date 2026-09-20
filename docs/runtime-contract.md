# Supported runtime contract

> P2 refinement (TALIA-3, 2026-09-20) supersedes the older policies below:
> extended values follow [engine values](engine-values.md); shared reads are the
> default and continue after their last waiter leaves. Stale getters reevaluate
> within reader deadlines; stale setters/actions fail without retry. SQLite
> commits precede successful writes; definition/state migrations activate
> atomically and fence old work. Historical P0 behavior remains regression evidence.


This records the approved P0 behavior for [TALIA-10](https://crumbles.lelloman.com/w/LLPR/TALIA/10).
It specifies the implementation target, not a claim that qualification is complete.
The historical [runtime](runtime-prototype.md), [execution](execution-policy-prototype.md)
and [transport](transport-prototype.md) reports retain their original scope.

## Runtime and portable formats

The P0 baseline is rquickjs **0.13.0** on Linux and Android, with its bundled
QuickJS-NG **0.16.2**, and **@jitl/quickjs-ng-wasmfile-release-sync 0.32.0** in a
separate capped WASM module and Worker per browser dashboard. The browser wrapper
is quickjs-emscripten 0.32.0 (the `quickjs-new` package alias). These distribution
versions do not imply identical engine builds. Bellard builds and the uncapped
browser variant remain regression comparisons, not the supported hosting strategy.

Exchange UTF-8 JavaScript source, never engine bytecode. The portable subset
includes functions, closures, classes, ordinary objects/arrays, Promises and
async/await exercised by the shared fixtures. Platform globals, DOM, Java APIs,
Node modules, arbitrary imports and direct network/filesystem access are not part
of the script API. UI authoring remains restricted JSX-like source compiled into
a typed tree; its grammar and renderer belong to P1.

Engine boundary values are plain JSON: null, booleans, strings, finite numbers,
arrays and string-keyed objects. Functions, accessors, cyclic graphs, undefined,
BigInt and non-finite numbers cannot cross that boundary. Numbers must respect
operation-specific precision/range checks; identifiers requiring exact integers
must fit the safe integer range or use strings. Native objects and promises are
not persistent state. Host adapters validate envelopes and values before effects.

## Authority and execution

Saved and live MCP-injected scripts are potentially faulty or hostile. The host
owns explicit grants, instance identity, generations and operation ownership.
Calling the raw bridge cannot expand authority or forge another instance's
identity. Engine operations are denied unless granted. Scripts never receive
credentials, filesystem access or arbitrary network access; configured host sources
and probes perform authorized I/O. View definitions are immutable to live scripts.
This is a JS boundary contract, not containment of arbitrary native code execution.

Reads, getters and setters may await; same-instance work continues during I/O.
Only short synchronous state updates are atomic. A stale evaluation fails with an
explicit error and cannot commit or publish success. There is no automatic retry;
a caller may explicitly initiate a fresh evaluation. Server updates are coordinated
at the authoritative server, frontend-local updates within that frontend instance.
No multi-variable transaction or rollback guarantee is implied.

Every computed definition explicitly selects shared or independent reads, with no
default. Shared readers join a compatible in-flight getter; setters remain free to
run. Cancelling one reader stops only its wait. Cancelling the last reader cancels
the producer. Cancelled work is skipped before start; running cancelled work cannot
commit, publish success or dispatch new effects, even after I/O completes. Previous
mutations and dispatched effects remain. Dependency cycles fail immediately.
Exact public API names, cache freshness, join parameter compatibility and definition
state migration remain separate API design questions.

## Dashboard lifecycle and failures

Android background and browser document-hidden transitions pause dashboard-local
JS and subscriptions. Server monitoring and already-dispatched actions continue.
Resume refreshes the snapshot and reconciles action outcomes without resubmitting
work. Old replies cannot resolve new-generation operations. Repeated transitions
must not leak subscriptions or block the UI.

A retained runtime preserves local state and temporary dirty ViewModel edits.
After process death, load the saved definition; temporary edits are lost. Pausing
is not a persistent save operation.

An internal script/runtime failure stops only the affected dashboard and releases
its subscriptions and pending waits. Show prominent diagnostics and a manual
**Restart** control. A configured failure signal may request an alert. Background,
resume and connection recovery never automatically restart a failed dashboard.
Restart creates a fresh saved baseline, discards temporary edits and preserves
server effects. Endpoint/probe/network failures are ordinary recoverable operation
or data errors, not fatal dashboard failures by themselves. An uncaught bug in the
dashboard's handling of such an error is still an internal script failure.

## Resource policy and provenance

Hosts must configure finite budgets for guest memory, synchronous execution,
source/message sizes, JSON complexity, pending calls, subscriptions, timers and
instance counts. Configuration belongs to the trusted host; guest scripts cannot
raise limits. Validate configuration before creating a runtime. Exceeding an
internal runtime budget stops the affected dashboard. I/O deadlines remain
operation errors. Do not hold a CPU deadline open across awaited I/O.

The recorded 16 MiB memory caps, 50 ms interruption probe, 32 KiB requests and
other fixture numbers are evaluation profiles, not production SLAs or universal
product defaults. Browser caps apply to the whole WASM module, not total browser
RSS. Native heap limits do not bound total process memory. Queue and host-side
allocation budgets require separate enforcement. Preserve uncapped regression
probes: the published browser runtime heap limit alone failed aggregate allocation.

Reproduce dependencies through the checked-in Cargo/npm lockfiles. Native engine
version comes from rquickjs-sys 0.13.0's bundled `quickjs/quickjs.h`; its bundled
`quickjs/LICENSE` and rquickjs's `LICENSE` are MIT. The browser package's manifest
and bundled `LICENSE` declare MIT and include upstream notices. Redistributed
artifacts must retain applicable notices. This is direct-runtime provenance,
not a completed release-wide dependency/license audit.

Existing evidence records Linux x86_64, Chromium 145.0.7632.6, Android 16/API 36
x86_64 emulator and one ARM64 OnePlus CPH2493. Android builds use NDK 27.0.12077973,
AGP 8.13.2 and Gradle 8.13. Runtime input manifests and transport manifests record
source/APK hashes. They prove only those recorded runs, not all Android versions
or browsers. Dependency upgrades require renewed qualification.

P0 qualification must cover actual host enforcement, lifecycle and manual recovery
on those four targets. Existing trusted helper tests alone do not establish those
guarantees. SQLite and server restart durability belong to P2; full notification
delivery to TALIA-7; production authentication/deployment and release-wide audits
to TALIA-8. P0 is not permission to deploy the unauthenticated fixture server.
