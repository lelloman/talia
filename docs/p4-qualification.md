# P4 MCP qualification

**Qualified on web and Android emulator on 2026-09-21.** All 17 required
checks passed, including 89 engine library tests, the MCP adapter frame test and
two native runtime tests. All five native integration suites verified app and
port-mapping cleanup. No physical device was used.

The reproducible runner is `engine/qualify-p4.py`; its execution record is
[engine/results/p4.json](../engine/results/p4.json). Qualification requires every
required check to pass against unchanged source bytes on web and a clean Android
emulator. Physical devices are rejected by the runner.

[TALIA-46](https://crumbles.lelloman.com/w/LLPR/TALIA/46) owns the runner,
qualification evidence and qualification fixes. The parent milestone is
[TALIA-5](https://crumbles.lelloman.com/w/LLPR/TALIA/5). Crumbles remains the
planning and review authority.

| P4 capability | Implementation commit |
| --- | --- |
| MCP contract, TALIA-38 | `f1319c6` |
| Atomic authored catalog/compiler, TALIA-39 | `329bebc` |
| Agent authority and audits, TALIA-40 | `5899833` |
| Persistent client registration, TALIA-41 | `4f6896a` |
| Saved delivery and assignments, TALIA-42 | `12cbf92` |
| MCP authoring, TALIA-43 | `96ad4f9` |
| MCP engine operations, TALIA-44 | `e0b3c42` |
| MCP live control, TALIA-45 | `3fea35e` |

## Acceptance evidence

| Acceptance | Executed coverage |
| --- | --- |
| External MCP configures monitoring and a shared interactive dashboard | `mcp_authoring --clients`: real stdio SDK negotiation, shared UI/VM/function references, Prometheus/HTTP fixtures, automatic Pipeline/Watch configuration, Chromium and native interaction |
| Conflicts, permissions and failed saves preserve coherent definitions | Rust authority/catalog tests and MCP authoring validation, transaction rollback, policy revocation and revision guards |
| Saved edits and instance parameters retain reference semantics | Authoring and saved-delivery suites: shared updates, independent assignments/parameters, updates available without adoption, explicit reload |
| Engine access and exceptional values | MCP engine reads/history/subscriptions, tagged values, guarded setters, durable Pipeline admission, deduplication and crash reconciliation |
| One live target cannot alter others or saved Views | Live suite runs two browser tabs and Android concurrently: browser edit leaves Android clean; Android edit leaves the other tab unchanged; isolated invocation has no UI/loaded VM globals |
| Dirty edits and dedicated reload | Live inspection stays clean; execution marks dirty; stale edit/assignment guards and explicit dirty acknowledgement; new instance identity and failed-guest restart |
| Cancelled/late work cannot dispatch effects later | Rust expiry, ownership, revocation and effect replay tests; real SDK cancellation after an effect; paused rejection and restart disconnection |
| Reload preserves effects and has a reconcilable outcome | Web/native live tests, server restart receipt persistence, saved-delivery/offline regressions, audit completion failure preserves earlier effects as unknown |
| Cleanup and provenance | Per-suite app absence and restored emulator reverse mappings; APK/shared-asset hashes; source hashes before/after execution; frozen P0 provenance check |

The aggregate record includes exact commands, outputs, exit codes, parsed suite
results, source hashes, the Android APK hash and packaged UI/VM/live/codec/native
hashes. Historical P0–P3 reports are preserved and are not relabelled as P4 evidence.

## Reproduction

Use the repository's existing cached Rust, Node, Gradle, SDK and emulator dependencies.
Start a disposable emulator in a separate terminal:

```sh
python3 engine/tests/emulator.py
```

Once `adb -s emulator-5570 shell getprop sys.boot_completed` returns `1`, run:

```sh
python3 engine/qualify-p4.py --emulator emulator-5570 --target-dir /tmp/talia-p3-target
adb -s emulator-5570 emu kill
```

The target directory is configurable and uses locked offline Rust dependencies.
The runner refuses a physical serial or an emulator with an existing Talìa app.
Every native suite installs its own app, removes it afterwards and restores port
mappings. The helper owns and deletes its temporary AVD when stopped. Tests use
temporary databases, credentials, fixture servers and browser profiles.

## Qualified scope and remaining product work

P4 is a loopback development implementation using external MCP agents. Live source
runs invocation-local JavaScript with `ctx.state/commit/read/write/run`; it can edit
root state but does not install persistent callbacks or replace loaded handlers.
Reusable behavior changes use saved authoring and explicit reload. These limits
are documented in [live control](mcp-live.md).

This qualification does not establish production readiness. The development-release
demonstration, Simple Agents/Crumbles task execution, alert delivery, production
operations and homelab migration remain separate roadmap stages. Review those stages
with the user before beginning further implementation.
