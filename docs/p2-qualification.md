# P2 durable engine qualification

This records the P2 baseline at `79d882c`. Current monitoring and client integration
checks are recorded in [P3 qualification](p3-qualification.md). References below to
current sources or evidence describe that preserved P2 baseline.

**Status: fully qualified on 2026-09-20.** Chromium, Android 36.1 x86_64
emulator and physical ARM64 OnePlus CPH2493 pass. No platforms are missing.
Both clients match on final observable state, and packaged shared assets match
the current sources. Test-app cleanup was verified by each native test.


Implementation lives in `engine/`, with web and native Android integration in
`dashboard/`. The first five subtasks have scoped commits and Review handoffs:

| Subtask | Commit |
|---|---|
| [TALIA-24](https://crumbles.lelloman.com/w/LLPR/TALIA/24), extended values | `d50149a` |
| [TALIA-25](https://crumbles.lelloman.com/w/LLPR/TALIA/25), SQLite recovery | `9ec7d4b` |
| [TALIA-26](https://crumbles.lelloman.com/w/LLPR/TALIA/26), runtime activation | `bc1bc77` |
| [TALIA-27](https://crumbles.lelloman.com/w/LLPR/TALIA/27), computed execution | `e4e92bd` |
| [TALIA-28](https://crumbles.lelloman.com/w/LLPR/TALIA/28), durable transport | `e4803a7` |

[TALIA-29](https://crumbles.lelloman.com/w/LLPR/TALIA/29) owns both-client integration,
qualification, and fixes discovered by those checks. The authoritative execution
report is [engine/results/current.json](../engine/results/current.json). Only its
`qualified: true` together with an empty missing-platform list constitutes complete
P2 qualification. An ARM64 build alone does not count as physical-device execution.

The runner records exact source hashes, command output, APK hashes, platform
results and native test-app cleanup. It tests real server process termination,
loss of an action response, crash after a setter's effect, database recovery and
backup/restore. The latter ambiguous action recovers as unknown and is not replayed.

Both clients exercise the same UI and ViewModel sources: exceptional values survive
QuickJS, SQLite and rendering; unavailable numeric controls expose their actual
value; charts annotate non-plottable values. Server restart preserves the live VM,
including temporary edits and selected screen. Connection indicators remain active
when the dashboard fails. Manual restart/reload restores the saved baseline.
Diagnostic `stateWire` preserves special values independently of lossy JSON display.

Regression coverage includes compiler/VM tests, web controls and rapid slider input,
responsive composition, saved updates, headed-browser lifecycle and native lifecycle/
renderer tests. P0 historical reports and sources remain unchanged and their
provenance is checked. Earlier P1 evidence describes its original committed baseline;
P2 records the current modified clients rather than rewriting historical results.

The power outage interrupted TALIA-26 before its commit and damaged an existing
build executable. Git/source checks and a clean target-directory rebuild recovered
the work. The final run uses `--target-dir /tmp/talia-p2-recovery-target`; the normal
reproduction command is documented in the [engine guide](../engine/README.md).

This is a loopback development engine. Production authentication/deployment,
collection adapters and schedules, Watches, notifications and full MCP integration
remain later stories. Current client grants cover the example `value` resource;
the server supports named instances. Migration work is bounded, not arbitrary
unlimited code execution; backing-device durability still depends on correct fsync.

The first physical P1 regression attempt lost foreground to Simple Agents. That
attempt is retained in the report; rerunning only the interrupted stage against
identical sources passed. The original P0 contract document is preserved byte for
byte so historical provenance remains valid; P2 policy changes are recorded in
the new engine contracts. [Artifact checks](../engine/results/artifacts.json)
verify shared assets for both APKs.
