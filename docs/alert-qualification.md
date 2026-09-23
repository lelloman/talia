# Alert qualification

The configurable alert implementation passed all 12 qualification checks on
2026-09-21. [Machine-readable evidence](../engine/results/alerts.json) records
commands, outputs, source SHA-256 hashes, APK and shared asset hashes, and cleanup
results. The run started from commit `a43e8d0` with the TALIA-55 changes included;
the recorded source hashes identify the tested inputs.

Run the qualification against a disposable Android emulator:

```sh
python3 engine/qualify-alerts.py --emulator emulator-5570 --target-dir /tmp/talia-p3-target
```

## Checks and outcomes

All engine tests passed (105 library tests and one MCP framing test), along with
the two native runtime tests and the shared JavaScript tests. Engine, web and
Android builds passed. Monitoring and MCP authoring regression tests passed.

The complete workflow used automatic Prometheus collection, policies authored
through the real MCP transport, and local SMTP, Telegram and FCM/OAuth fixtures.
It verified dashboard-only warnings, critical-stage delivery through all three
providers, independent Telegram rate-limit retry, global acknowledgement,
recovery and history, silence expiry, explicit occupied-stage migration, and
state preservation after a crash and restart.

Web controls passed in Chromium. Native controls and notification behavior passed
on an x86_64 Android emulator: installation registration, push token rotation,
identity preservation across restart/update, notification expiry, direct
acknowledgement, visible offline acknowledgement failure, fetching resolved state
when opening a notification, and a new identity after clearing app data.
The APK's shared UI and ViewModel assets matched the tested source. The release
manifest excludes the debug notification injection activity.

Qualification exposed and fixed two scheduling cases: an overdue repeat now
advances its next deadline from actual dispatch time, and a missing destination
records its error without preventing unrelated destinations from being scheduled.
Both have regression tests. Android notification assertions now wait for the
platform to post the notification before inspecting it.

The test application was removed and reverse mappings restored after both Android
suites. The disposable emulator was stopped after qualification. No physical
device was used.

## Deployment boundary

Provider delivery was tested against local fixtures only. No real messages were
sent, no Firebase project or production credentials were provisioned, and nothing
was deployed. Live SMTP/Telegram delivery and Firebase token provisioning/cloud
receipt remain deployment qualification tasks. Retries do not promise exactly
once external delivery when provider acceptance is uncertain.

See [alert configuration and operational limits](alerts.md) and the
[implementation plan](implementation-plan.md). Release/deployment preparation
precedes homelab migration; LLM investigations and Crumbles integrations follow that
migration.

## Browser Web Push fixture coverage (TALIA-66)

`cargo test --manifest-path engine/Cargo.toml --offline` includes real VAPID
signing and encryption/decryption against a local HTTP fixture, acceptance and
failure status mapping, destination-channel isolation, subscription retirement
without disabling a rotated address, ownership and stale-alert checks.
`node dashboard/tests/access.mjs` runs Chromium against the real HTTPS/OIDC
service, with only the vendor subscription mocked, to exercise browser enrollment,
root service-worker registration, endpoint rejection, viewer denial and disable.
`node dashboard/tests/browser-push.test.mjs` covers worker display, stale/denied
suppression, generic offline fallback and fixed same-origin click navigation.
These checks do not establish real browser-vendor or OS background delivery.
