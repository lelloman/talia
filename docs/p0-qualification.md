# P0 qualification status

The combined source-qualified run on 2026-09-19 passed on Linux x86_64,
Chromium 145.0.7632.6, an Android 16/API 36 x86_64 emulator and a physical
ARM64 OnePlus CPH2493 on Android 16/API 36. **The four-platform qualification
gate passes; P0 remains subject to review and acceptance.** Evidence is owned by
[TALIA-15](https://crumbles.lelloman.com/w/LLPR/TALIA/15).

The [runtime contract](runtime-contract.md) defines the target. The current
[evidence](../spikes/qualification/results/full-2026-09-19.json) records source hashes,
base commit, build/run commands, runtime versions and installed APK/library hashes.
The [qualification runner](../spikes/qualification/README.md) documents repetition
and validation. The full report records `qualified: true` with no missing platforms.
The earlier `results/current.json` remains unchanged as historical evidence of the
three-platform run; it deliberately still records `qualified: false`.

| Contract area | Current evidence |
|---|---|
| Explicit host capability grants and protected fixture Views | 16 matching saved/live bypass cases, dispatch rechecks and granted operations across all four hosts |
| Async execution, shared reads and cancellation | 27 shared async checks, 21 matching trusted-authority checks, raw cancelled/stale effect rejection and protected state access |
| Fault isolation and resource limits | Native/process/service and capped Worker recovery regressions; unrelated runtime and UI progress |
| Real client/server transport | 25 matching checks across all four hosts plus 10 server validation/concurrency cases |
| Background/hidden pause and resume | Actual Chromium tab switches and Android Home/foreground transitions, server action continuation, reconciliation, retained dirty state and process/reload baseline restoration |
| Internal error versus external failure | Script error, runaway and OOM stop the affected dashboard; external probe errors remain recoverable; visible diagnostics, opt-in fixture signal and manual Restart pass in both clients |
| Physical ARM64 | Fresh installed APK/library hashes, runtime/service recovery, transport, lifecycle, internal/external failure and actual Restart-button tests all pass |

These are qualification adapters, not the production application. Shared UI
compilation/rendering is P1; durable server definitions/state and crash recovery
are P2. Full alert delivery is TALIA-7, production authentication/deployment and
release-wide license review TALIA-8. No deployment or homelab migration occurred.
The native lifecycle Activity handles guest errors in-process; separate service
process tests cover injected native abort/hang containment. The combined evidence
does not establish a production integration of those two paths or containment of
arbitrary native exploits. Budgets do not bound total process/browser RSS.

Prior experiment reports remain unchanged in their original locations. Their
source-hash validators correctly reject changed source revisions; use the combined
qualification report for this revision. Future runs should preserve this report
with a distinct output path when collecting newer evidence.
