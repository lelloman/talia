# P3 monitoring qualification

**Status: qualified on web and Android emulator on 2026-09-20.** All 21
required checks passed, including 33 engine tests and two native runtime tests.
No requested platforms are missing. All three native suites verified app cleanup.
Physical-device testing is excluded at the user’s request.

P3 implements automatic collection, reusable stateful Watches and investigation
Pipelines, with shared web and native Android dashboards. The authoritative
execution record is [engine/results/p3.json](../engine/results/p3.json). Completion
requires `qualified: true`, no missing platforms and successful required checks;
the requested scope is web and emulator. Physical-device testing is opt-in and
was stopped at the user’s request; it is not required for this qualification.

| Subtask | Commit |
|---|---|
| [TALIA-30](https://crumbles.lelloman.com/w/LLPR/TALIA/30), definitions and persistence | `ba4eb45` |
| [TALIA-31](https://crumbles.lelloman.com/w/LLPR/TALIA/31), Prometheus and HTTP adapters | `65c06d6` |
| [TALIA-32](https://crumbles.lelloman.com/w/LLPR/TALIA/32), durable Pipeline execution | `80a60b9` |
| [TALIA-33](https://crumbles.lelloman.com/w/LLPR/TALIA/33), scheduling and freshness | `e9c49dd` |
| [TALIA-34](https://crumbles.lelloman.com/w/LLPR/TALIA/34), stateful Watches | `bd3f3f9` |
| [TALIA-35](https://crumbles.lelloman.com/w/LLPR/TALIA/35), monitoring API | `05a98e1` |
| [TALIA-36](https://crumbles.lelloman.com/w/LLPR/TALIA/36), both-client integration | `a88b4e9` |

[TALIA-37](https://crumbles.lelloman.com/w/LLPR/TALIA/37) owns the qualification
runner, evidence and fixes discovered during qualification. Crumbles remains the
authoritative plan and review queue.

## Coverage and provenance

The runner records exact engine/dashboard source hashes, commands and output,
APK hashes, packaged shared assets, platform results and native app cleanup.
Rust tests cover atomic activation, migrations, scheduling, ordered Watch
observations, durable execution and cancellation. Fixture-backed HTTP tests
exercise collection without clients, disk thresholds, investigation dispatch,
restart recovery, subscriptions, admission deduplication and explicit unknown
outcomes after interrupted effects.

The shared monitoring package displays CPU, memory and disk metrics, freshness,
Watch flags and investigation results. Browser and native checks exercise all nine
subscriptions, explicit investigation, temporary VM edits, server disconnect and
recovery, and dashboard reload. Android backgrounding pauses local work while
server collection continues. Existing P1 and P2 suites cover controls, responsive
composition, saved updates, lifecycle, exceptional values and durable transport.
P0 source provenance is checked without changing its historical sources or report.

Qualification identified and fixed four edge cases: cancellation of queued work
must not invalidate the active run; removing and recreating an instance must fence
its old in-flight work; callbacks from a closed browser bridge must not produce an
unhandled rejection; legacy Android fixture requests must omit named-resource
arguments. Focused Rust regressions cover the first two; full client suites cover
the latter two. Earlier qualification attempts failed before these fixes; the
recorded final run describes the resulting source bytes. The fresh-emulator
pass also exposed test timing issues: hierarchy capture now retries bounded
transient failures, and manual-investigation checks wait for a new completed run
ID rather than accepting the previous run’s status.

## Reproduction

With an emulator connected:

```sh
python3 engine/qualify-p3.py --emulator EMULATOR_SERIAL --target-dir /tmp/talia-p3-target
```

`--resume` accepts only identical source hashes. `--rerun CHECK...` forces selected
checks; rerun the corresponding Android build when changing ABI. Native fixtures
remove their test app and ADB reverse mapping after execution. The runner reports
a missing emulator as incomplete qualification. Add `--physical PHONE_SERIAL`
only when physical-device testing is explicitly requested. An ARM64 build alone
does not count as physical-device execution.

## Scope

These are isolated Prometheus/HTTP fixtures and a loopback development service;
existing homelab monitoring is untouched. Production authentication, deployment,
full MCP integration, Crumbles/SimpleAgents execution and notification delivery
remain later work. Prometheus native histograms are explicitly unsupported;
calendar schedules currently support daily times with optional weekdays.
See the [monitoring contract](monitoring.md) for operational limits and semantics.
