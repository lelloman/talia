# Talìa Android

Native Compose application shell with LelloDesign's actual scaffold, drawer,
account and section components. Appearance lives in Settings and persists across
launches. Paravoid's shell-owned update controls are accessible from Settings.

Native sign-in and the live Overview are implemented, with encrypted session
storage, service sample freshness, recent reports, and explicit failure states.
They require the matching native API endpoints to be deployed. Report execution,
chat, automation and the Rust runtime are not connected yet.
See `../docs/android-native-api.md` for authentication, scope and validation.
The gateway integration and new shell onboarding are documented in
[`android-remote-access.md`](../docs/android-remote-access.md).

## Build

Requirements: JDK 17, Android SDK 36 with build-tools 36.1.0, Android API 30+ device, sibling
`../../paravoid-android` and `../../lellodesign/packages/compose` checkouts.
Override them with `PARAVOID_SOURCE_DIRECTORY` and
`LELLODESIGN_COMPOSE_DIRECTORY` (absolute paths recommended). The app consumes
LelloDesign through Gradle composite substitution, not copied Compose components.

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
./gradlew :app:assembleNormalDebug
../scripts/publish-android-to-lellostore.sh --build-only
../scripts/publish-android-to-lellostore.sh --dry-run --json
```

The normal package is `com.lelloman.talia.normal`; the production Paravoid package
is `com.lelloman.talia`, version 0.1.2 (3), payload version 5. They can be
installed side by side. Paravoid uses complete packaging, embedded bootstrap,
APK-grant Store delivery, explicit update controls and default crash recovery.
No automatic check/download schedule is enabled. Payload-only R8 shrinking,
optimization and obfuscation are enabled via `minifyPayload`. Standard AGP
minification and resource shrinking remain disabled for Paravoid compatibility.
The build collects external dependencies' consumer rules and applies
`app/payload-rules.pro` for reflection entry points. Keep the exact
`payload-mapping.txt` alongside each released VPK for crash decoding.

## Signing and trust

Create the local app identity once:

```sh
../scripts/setup-android-signing.py --store-trust /path/to/store-public-trust.json
```

This imports only the Store's public head/grant keys, generates separate APK
and VPK RSA-3072 keys, and writes private configuration under
`~/.config/talia/paravoid-release/` and ignored `signing.properties`. It refuses
to overwrite existing configuration. Back up these files securely; loss of the
APK key prevents compatible APK updates. Do not commit keys or passwords.
If interrupted during initial generation, inspect the partial files before any
manual repair; the tool deliberately does not silently regenerate identity.

The new APK 3 baseline is pinned in `paravoid-baselines/apk-3` and used by
default. APK 2's published baseline is retained in `paravoid-baselines/apk-2`. Payload 3 adds native sign-in and Overview. APK 2 has no browser callback
intent filter: after signing in, switch back to Talìa to finish connecting. The
normal build retains the callback for development.

For a new shell generation, export and review a complete shell baseline:

```sh
./gradlew :app:exportParavoidAndroidReleaseParavoidCompleteBaseline
```

Retain the accepted contract outside build outputs, pass
`-PparavoidBaselineDirectory=/path/to/baseline` and increase
`-PparavoidPayloadVersion=N`. New manifest capabilities, trust or shell runtime
changes require a new shell generation. Increase Android versionCode/versionName
before uploading a new APK. These values are never incremented by the publisher.

See `../docs/android-publishing.md` for prebuilt shell and payload uploads and
`../docs/android-app-flows.md` for the implementation plan.

## Initial validation (2026-09-27)

Normal debug and signed Paravoid release builds passed. The shared LelloStore
publisher accepted the 22,881,175-byte shell APK with `--dry-run --json` (status
`valid`); nothing was uploaded. Both packages launched on the API 36.1 emulator.
The LelloDesign drawer and Settings navigation worked, and dark appearance
survived a full restart of the signed app.

Paravoid's update controls on the directly installed local shell report
`UPDATE_SERVICE_UNAVAILABLE`. Investigation confirmed the installed APK matches
the local artifact and `ApkGrantReader` rejects it with `CREDENTIAL_UNAVAILABLE:
APK update credential missing`. With `authentication = 'apkKey'`, LelloStore
inserts the credential during authenticated acquisition; the build artifact
does not contain one. UpdateRuntime aborts at `DeliveryClient.installedApk` and
UpdateService replaces the underlying error with the generic message. This
happens before an update request reaches LelloStore.

Version 0.1.0 (1) and its embedded payload were subsequently published on
LelloStore, publication revision 2. The live Store reports matching public
head/grant keys, verified shell and payload, Paravoid distribution mode, and an
active stream. An authenticated acquisition produced a personalized APK with
the grant present; its download hash matched the Store's response.

Installing that APK over the local shell on the API 36.1 emulator removed the
service error. Controls show installed version 1, and a manual network check
successfully discovers the published payload. The UI offers version 1 even
though version 1 is already installed; choosing download returns to that same
offer. A newer-version download/apply cycle has not been validated.

Use authenticated LelloStore acquisition for update testing; a raw/admin APK
download is not a substitute. The build uses sibling Paravoid source at commit
`6770678`.

## Payload 3 release validation

Payload 3 is published and verified on LelloStore (publication revision 6),
targeting the existing APK 2 contract. The compatibility check passed against
the committed baseline. An emulator installed the authenticated Store APK 2,
discovered payload 3, downloaded it, and applied it via the restart controls.
The updated app opened the native server/sign-in screen successfully.
Interactive production authentication was not exercised in this release check.

## Minified payload 4 release

The signed VPK is 3,302,404 bytes (payload 3 was 22,436,062 bytes), an 85.3%
reduction. Release build and APK 2 contract compatibility checks passed.
A separate emulator user launched the embedded minified payload, initiated
browser sign-in, restored encrypted pending sign-in across a cold start, and
retained dark appearance after another cold start. The crash buffer was empty.
Full authenticated Overview/sign-out and a Store-delivered upgrade from payload 3
are still untested. Payload 4 is published and verified on LelloStore (publication revision 8),
replacing payload 3 on the APK 2 stream. The exact VPK, R8 mapping and source
commit provenance are archived locally under `.local/android-releases/payload-4`
at the repository root.

## Overview and Settings refresh (payload 6)

Overview uses LelloDesign workspace spacing, semantic status colors, a service
summary panel, compact service rows and expandable report panels. A completed
report run is labeled separately from the service health described by its result.
Stale samples remain visibly stale. Setup and account actions live in Settings;
Appearance uses the shared selector. Page scroll and expanded details survive
navigation, and larger text uses stacked counts and service rows.

Light/dark and 150% text-size fixture screenshots were visually reviewed.
Twelve device tests (including existing authentication/gateway regressions),
Android lint and signed minified build checks passed against the APK 3 contract.
The fixture data exists only in androidTest. Payload 6 is published and verified
on LelloStore (publication revision 12), on the active APK 3 update stream.
The signed VPK is 3,760,741 bytes, built from source commit `17f655b`.
The exact payload, R8 mapping and provenance are archived locally under
`.local/android-releases/payload-6` at the repository root.

## Reports (payload 7)

The native Reports screen provides the report catalog, paginated run history,
manual preview execution and automatically refreshed run details using shared
LelloDesign components. Runs do not send Telegram/email notifications. Admission
request IDs are encrypted and persisted before dispatch; explicit retry recovers
the same run after an ambiguous response or app restart. The selected report/run
also survives restarts. Reports requires the backend's `/native/reports` endpoint
and current administrator access. See [report behavior](../docs/reports.md).

Validation: all 16 emulator tests passed, including native/gateway regressions,
admission retry after restart, history pagination, permission revocation and
catalog/history/result rendering. Catalog and result screenshots were reviewed.
Android lint and the signed minified APK 3 compatibility build passed. Backend
regression tests passed (151 library tests, 26 service tests, one CLI test; one
pre-existing ignored service test).

Payload 7 is published and verified on LelloStore (publication revision 14),
on the active APK 3 stream for `com.lelloman.talia` / APK 0.1.2 (3).
The signed VPK is 3,782,833 bytes, SHA-256
`19b1954f1dbb0e50bb117b8dc73e43f1401d25296b73b6debf0609f7436956c9`.
Artifacts, R8 mapping and provenance are archived in
`.local/android-releases/payload-7`. The signed minified shell opened Reports
on the emulator without a crash. A Store-delivered upgrade and authenticated
production report execution remain phone acceptance checks.

Backend commit `06d59a3` is deployed to homelab. Health returned 200 and the
Reports endpoint returned 401 without a session, both internally and through
normal HTTPS routing. Both SQLite databases were backed up and passed integrity
checks before deployment; container-local copies are under
`/data/backups/reports-20260928`. No schema migration or gateway change was needed.

## Compact Reports and browsing controls (payload 8)

Catalog/history cards are replaced with compact, full-width tappable rows.
The catalog adds name search, newest/oldest/name sorting and status/schedule
filters. History adds newest/oldest sorting and status filters across the entire
server history, preserving those selections for subsequent pages. Changing a
history control starts a fresh page. Execution details remain available on tap.

Validation: 18 emulator tests passed, including catalog search/sort/filter
combinations, dropdown interaction and history cursor reset/preservation.
Eight backend report regressions passed, including filtered ascending pagination.
Compact catalog and history screenshots were reviewed; Android lint and the
signed minified APK 3 compatibility build passed.

Payload 8 is published and verified on LelloStore (publication revision 16), on
the active APK 3 stream. The VPK is 3,841,101 bytes, SHA-256
`bf7d4a03c4caf54bd9a199bba8e8929283ddfde72f5ac0d073d05610b218c3b8`.
Exact artifacts, R8 mapping and provenance are archived in
`.local/android-releases/payload-8`. Backend source `ed2145d` is deployed and
healthy. The signed shell launched on the emulator without crash output.

## Reports pull-to-refresh (payload 9)

Reports no longer repeats the title, subtitle and Refresh button below the app
bar. The selected report name lives in the app bar. Material 3 pull-to-refresh,
using the LelloDesign theme, refreshes catalog, history and run details. A custom
accessibility action also exposes refresh. The viewport remains scrollable for
short or empty lists, and signed-out setup does not issue refresh requests.
All 18 emulator tests passed, including actual downward touch gestures on all
three Reports screens. Lint and signed minified APK 3 compatibility checks passed.

Payload 9 is published and verified on LelloStore (publication revision 18),
on the active APK 3 stream. The VPK is 3,862,165 bytes, SHA-256
`ffb3935540edaa7de07e7d29fcf8db3c661fd051477e4b52b9147b87272caf07`.
Source commit: `0e98386`; artifacts, R8 mapping and provenance are archived in
`.local/android-releases/payload-9`. No backend deployment was required.

## Shared LelloDesign browsing controls (payload 10)

Reports now consumes `LelloListControls` from the sibling Compose library at
LelloDesign commit `8f66a1f`, replacing Talìa's custom `ReportChoice` dropdown.
Sort is a neutral action showing the current order. Filter opens the reusable
sheet, with draft selections committed only by Apply. Applied criteria become
removable chips. Catalog status and schedule criteria can be combined; history
continues to apply its filter on the server before pagination. Search and
pull-to-refresh remain intact.

The library's three interaction tests, unit tests and lint passed. Talìa's 18
device tests, lint and signed minified APK 3 compatibility checks passed; shared
sheet and applied-filter screenshots were reviewed.

Payload 10 is published and verified on LelloStore (publication revision 20),
on the active APK 3 stream for `com.lelloman.talia` / APK 0.1.2 (3).
The VPK is 3,946,165 bytes, SHA-256
`c1f82358b4758e6b8122f17f71ed3b33893a2860a79a9639681c2cac114303e5`.
Source commit: `e489730`; artifacts, R8 mapping and provenance are archived in
`.local/android-releases/payload-10`. No backend deployment was required.

## Report scheduling (payload 11)

Reports history now shows the configured schedule and server-owned next run.
Edit schedule opens a LelloDesign form for daily time, IANA timezone and weekdays,
or an elapsed interval. Enable/pause preserves timing; None removes it. The form
shows existing delivery destination IDs. Scheduled runs send to those destinations;
manual Run report stays an unsent preview. Pausing does not cancel admitted runs.

Schedule saves use version checks and durable request IDs. An unconfirmed save is
persisted in the encrypted session and can be checked after restart without
applying it twice. A concurrent edit requires refreshing and reviewing the current
schedule. Both Android and the web Reports page share the new backend operations;
deploy the backend/web update before installing this payload. No migration or new
shell APK is required. Android notifications remain a later slice.

Validation: 20 Android device tests, lint, web interaction checks, report API and
scheduler regressions passed. The editor was reviewed on the emulator and at a
390px web viewport. Signed minified payload 11 is compatible with APK 3.

Payload 11 is published and verified on LelloStore (publication revision 22),
for `com.lelloman.talia` / APK 0.1.2 (3). The VPK is 3,968,409 bytes, SHA-256
`642499686db0e620823952a3ac1696ff5ca323999ec4e88fc3b454f881a5d83e`.
Source commit: `67778eb`; artifacts, R8 mapping and provenance are archived in
`.local/android-releases/payload-11`. Backend/web revision `67778eb` is deployed
and healthy. Both SQLite databases were backed up and passed integrity checks
before deployment, under `/data/backups/schedules-20260929` in the container.

## Bottom navigation (payload 12)

Phones use LelloDesign's DrawerAndBottom scaffold with Overview, Reports and
Chats tabs. Settings and Automation remain accessible from the drawer. Chats
continues to show its existing placeholder. Wide layouts retain the sidebar.
Debug build, Android lint and signed minified APK 3 compatibility checks passed.

Payload 12 is published and verified on LelloStore (publication revision 24),
for `com.lelloman.talia` / APK 0.1.2 (3). The VPK is 3,968,733 bytes, SHA-256
`24a498c87aa570869283123069665b64b0f0eb4330670cf2c52db8ef4efa00d2`.
Source commit: `149eccf`; artifacts, R8 mapping and provenance are archived in
`.local/android-releases/payload-12`. No backend deployment was required.
