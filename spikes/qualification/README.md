# P0 conformance and qualification

Run from the repository root after installing the pinned runtime npm dependencies
and Playwright Chromium. Android builds require SDK 36, NDK 27.0.12077973, Rust
Android targets and Gradle 8.13 (`TALIA_GRADLE` can override discovery). Actual
browser visibility checks use Xvfb and Chromium's direct debugging protocol.

```
python3 spikes/qualification/run.py --android emulator-5558 --physical PHONE_SERIAL --output spikes/qualification/results/recheck.json
python3 spikes/qualification/check.py spikes/qualification/results/recheck.json
```

The runner builds current sources, starts its own ephemeral loopback server, runs
Linux/runtime/process/transport and browser/runtime/transport/lifecycle/failure
checks, then builds and tests each Android ABI. Android checks install only the
fixture package and remove it and its ADB reverse mapping in `finally` blocks.
The runner stops its own server; the browser collector stops its own Chromium and
Xvfb. It does not stop user-owned emulators. ADB devices must remain connected
through cleanup. The native collector checks the installed APK hash and packaged
library; the lifecycle collector reinstalls that same artifact.

The result records source hashes, base commit, build/run commands, tool versions,
APK provenance and fresh per-platform reports. Hashes must match before and after
execution and during validation. Historical experiment reports are never rewritten.
A report with a missing physical phone is explicitly unqualified and exits 2:

```
python3 spikes/qualification/run.py --android emulator-5558
python3 spikes/qualification/check.py spikes/qualification/results/current.json --allow-missing-physical
```

The second command validates the available evidence only; it does not pass P0.
Routine changes to runtime/transport/lifecycle code require this combined run,
not just the older historical report validators. Use a distinct `--output` path
to retain previous qualification runs when requalifying a later revision.

## Scope and limits

The checks cover 14 runtime behaviors, 27 async helper behaviors, 16 capability
bypass cases, protected host execution authority, resource/fault containment,
25 real HTTP transport behaviors, 10 server validation cases, real client lifecycle
transitions, visible internal failures and manual saved-baseline recovery.
Legacy uncapped browser memory failures remain intentional regression evidence;
only separate capped modules are the supported browser baseline.

The lifecycle shell is a qualification adapter with snapshot polling and fixed
loopback namespaces, not the shared P1 renderer. Protected execution uses a
host-bound operation per guest; its public API and integration into the durable
server remain later work. Failure signals feed a local fixture receiver, not a
production notification service. Native service-process crash containment is
exercised separately from the in-process lifecycle Activity. These tests do not
prove containment of arbitrary native code exploits or bound total process RSS.

P0 remains open while any required platform or agreed boundary is unqualified.
SQLite/server restart durability is P2; production auth/deployment and complete
release license auditing are TALIA-8; full alert delivery is TALIA-7. No production
deployment or homelab cutover is performed by this qualification command.

## Completed four-platform run

`results/full-2026-09-19.json` passes the default validator and records
`qualified: true` on Linux, Chromium, Android x86_64 emulator and physical ARM64
OnePlus CPH2493. The earlier partial `results/current.json` is preserved unchanged.
See [P0 acceptance scope](../../docs/p0-qualification.md); passing this fixture gate
is ready for review, not a production deployment approval.
