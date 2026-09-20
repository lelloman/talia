# P1 shared dashboard qualification

**Status: fully qualified on 2026-09-20.** P1 checks pass on Chromium,
Android x86_64 emulator and the physical ARM64 OnePlus CPH2493. Both Android
ABIs build successfully and contain the same shared UI and ViewModel sources.
Fresh P0 regressions pass on Linux, Chromium, Android x86_64 and physical ARM64;
the older four-platform P0 report still matches its unchanged source inputs.

The shared dashboard implementation is in `dashboard/`. Its seven implementation
subtasks have separate commits and review handoffs. [TALIA-23](https://crumbles.lelloman.com/w/LLPR/TALIA/23)
owns the final qualification harness, lifecycle fixes and recorded evidence.

| Subtask | Repository commit |
|---|---|
| [TALIA-16](https://crumbles.lelloman.com/w/LLPR/TALIA/16), contract | `c74acb8` |
| [TALIA-17](https://crumbles.lelloman.com/w/LLPR/TALIA/17), compiler | `9878779` |
| [TALIA-18](https://crumbles.lelloman.com/w/LLPR/TALIA/18), bindings | `46be81b` |
| [TALIA-19](https://crumbles.lelloman.com/w/LLPR/TALIA/19), web | `bac6f19` |
| [TALIA-20](https://crumbles.lelloman.com/w/LLPR/TALIA/20), native Android | `94b6175` |
| [TALIA-21](https://crumbles.lelloman.com/w/LLPR/TALIA/21), responsive composition | `45b9868` |
| [TALIA-22](https://crumbles.lelloman.com/w/LLPR/TALIA/22), saved updates and reuse | `5062c89` |

## Acceptance boundary

[Current evidence](../dashboard/results/current.json) records exact source hashes,
commands, package revision, APK hashes and individual platform outcomes. Its
`qualified` field is true, with no missing platforms. Historical
P0 reports are preserved; the runner also verifies their source provenance and
can write fresh P0 regression evidence alongside the P1 report.

The shared example includes persistent navigation and content Screens, a chart,
subscribed server data, slider/switch/button actions, repeated content and shared
UI references. Tests compare final ViewModel state across hosts, rather than
pixel equality. Actual browser visibility and Android Activity transitions test
pause/resume and in-flight effects. Internal exceptions, runaway CPU and guest
memory exhaustion require manual restart and retain server effects.

Native renderer checks exercise real attached Android Views for keyed identity,
focus, hidden/collapsed layout, long text, grid columns and accessible labels.
Web tests cover keyboard controls, continuous slider dragging, keyed DOM focus,
explicit manual dp scaling, per-client composition, saved revisions and temporary
VM edits. Both clients verify persisted dashboard selection. Android never
uses a WebView. The tested emulator uses Android 36.1/x86_64; wider OS/browser
coverage is not implied by this evidence.

The qualification pass also added bounded native I/O dispatch, cancellation checks
before queued effects, guest retirement on internal failure, exception diagnostics,
and an authored resume hook after outcome reconciliation. These address lifecycle
behavior in the new clients; they do not alter historical P0 experiment inputs.

## Reproduction and cleanup

See the [dashboard guide](../dashboard/README.md). Run
`python3 dashboard/qualify.py --emulator SERIAL --physical PHONE_SERIAL --p0-regression`
with the phone connected and unlocked. The runner checks that packaged shared
artifacts match source, compares final UI state, and records test-app cleanup.
Without a physical device it reports that platform as missing and exits 2.
The fixture is local development infrastructure; durable production storage,
authentication, complete MCP tools, notifications and deployment remain later
Crumbles stories.
