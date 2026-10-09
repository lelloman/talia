# Homelab monitoring

Published 2026-10-07. Talìa owns 40 monitoring conditions: all 29 former Prometheus
alert rules plus 11 conditions derived from the hourly host health probe. Each
condition has a scheduled Prometheus query and a separate input-availability alert.
The existing Git workspace policy is unchanged: 81 bindings total in production.

`rules.json` is the reviewable source inventory, including original PromQL,
threshold durations, collection intervals, severity and message templates. The
US security heartbeat expression now detects missing collectors on both VPSes.
Prometheus remains the scraper/time-series store; it no longer evaluates alert rules
or forwards them to Alertmanager. No Talìa binary or dashboard rebuild was required.

## Native definitions

- `collect.js`: bounded vector queries, finite-value checks, explicit rejection of
  partial/warning results, original labels and sampled values.
- `condition.js`: independent pending duration per label set, latched firing until
  a fresh query proves recovery, and reset pending durations after collection gaps.
- `availability.js`: a separate warning after two minutes of unavailable/stale input.
- `deploy.py`: version-guarded MCP publishing, live definition backups, existing
  approved destination lookup, and shadow/notification configuration.

One Talìa occurrence groups all affected series for an alert name, matching the
previous Alertmanager grouping by alert name/severity. Acknowledgement therefore
applies to that group. New/changed affected membership changes stage and resets
acknowledgement so new problems are not hidden behind an earlier acknowledgement.
Full label/value evidence is in `engine_read` on `infra-ALERT_NAME`; notification
messages are bounded. Critical alerts notify immediately and repeat every four hours;
warnings wait 30 seconds and repeat every twelve hours. Acknowledgement stops repeats.
Recovery sends once. Delivery retries, silences and history are owned by Talìa.
Failed SSH authentication bursts are monitoring-only (`notify: false`): both
firing and recovery messages are suppressed while measurements and alert history
remain available. Unexpected successful logins, security-file changes, and missing
SSH monitoring data retain their notifications. Existing bindings preserve live
overrides on redeployment; update their parameters explicitly to apply this default.

These are native queries and policies, not an Alertmanager forwarding bridge.
Missing data freezes the affected condition and surfaces the separate input warning;
it cannot silently resolve an incident. The host-health producer also has a 65-minute
stale deadline, independent of Prometheus's ability to scrape its old textfile.

## Publishing

Run from a machine with the established `ssh homelab` operator access. The publisher
uses `docker exec talia talia-mcp` with the mounted operator credential; it never
reads that credential into command output or committed files. It requires exactly
one enabled, approved Telegram destination.

```sh
node deploy/homelab-monitoring/test-policies.mjs
python3 deploy/homelab-monitoring/deploy.py --backup /private/new-shadow-backup
# After comparing shadow results, execute the host cutover and enable delivery:
python3 deploy/homelab-monitoring/deploy.py --enable-delivery --backup /private/new-cutover-backup
```

Default publishing disables delivery for the migration policies, including on an
already-live installation. Do not run it casually as a status check. Existing binding
parameters/state are preserved. Definition differences fail unless explicitly allowed
with `--update-definitions`; inspect the backed-up current definitions first. All
new writes use current catalog/policy versions and recorded request IDs. On uncertain
MCP outcomes, inspect `operation_status` before retrying.

Production deployment files, health.db compatibility, the independent Talìa-outage
watchdog, verified findings, backup locations and rollback are documented in the
[homelab cutover runbook](../../../homelab/talia/migration/README.md).

## Evidence

Pure-policy checks cover threshold duration, independent label sets, new affected
members, immediate SSH alerts, recovery, sampling gaps and unavailable input. The
live engine validated all catalog changes and ran the native rules in shadow before
delivery was enabled. All 81 evaluations were error-free after cutover. Real active
incident and recovery notifications were recorded as `sent`; no synthetic delivery
test was submitted. Existing host dashboards/reports and their assignments were preserved.

Known pre-existing probe limitations now appear as findings: broken Promtail Docker
discovery, insufficient host access to LelloAuth's DB, and a Favzetto read-only SQLite
failure. The new US security collector preserves its initial historical detections
while allowing only the documented Meteonesto sync account/source for future checks.
