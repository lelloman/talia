# Infrastructure checklist reports

[Definitions](homelab-telegram-reports.json) run at **09:00, 14:00 and 19:00
Europe/Rome**, including daylight-saving changes. Captured versions are **7, 6, 6**.

The checklist contains Homelab, VPS-EU, VPS-US, LelloAuth, Knot Resolver,
Pezzottify, Pezzottflix, SimpleAI, Simple Agents, LelloStore, Crumbles, Backups,
and TLS certificates. Observo is excluded. Host checks aggregate their metrics;
services have their own entries instead of being repeated under Homelab.

Healthy items display only `✅ Name`. Anomalies display `⚠️ Name — WARN` or
`❌ Name — ERR`, measured problems, and a short item-specific AI assessment.
The headline is Nominal, Warning or Error according to the worst measured item.

## Deterministic checks

- Hosts: current exporter reachability, CPU and memory (warning at 90%), disk free
  (warning below 10%), rolling 24-hour scrape availability (warning below 99%).
  A down exporter is Error, not proof that the host itself is offline. Expected
  mounts are `/`, `/boot/efi`, `/mnt/external` on Homelab and `/` on each VPS.
  All other reported persistent mounts are checked too. Missing required data is
  Warning. Dependent current measurements are suppressed when the host exporter
  is down/unknown. VPS systemd services explicitly reporting failed state are Error.
- Per-host internet: separate external DNS and HTTPS probes must originate from
  that host; missing probes are Warning. A probe failure is described specifically,
  not as proof of a general internet outage.
- LelloAuth, Knot Resolver, Pezzottify, Pezzottflix, SimpleAI and Simple Agents:
  current scrape reachability and rolling availability. Down is Error; missing
  reachability/availability is Warning. Scraping does not establish full application
  health or successful authenticated user workflows.
- Knot Resolver: SERVFAIL response ratio over 15 minutes warns above 1%.
- Pezzottify: HTTP 5xx ratio over 15 minutes warns above 1%; unavailable ratio warns.
- SimpleAI: `simpleai_up` must be 1; this does not run a synthetic inference.
- Simple Agents: uncertain effects or exhausted recovery above zero warn;
  unavailable metrics warn.
- LelloStore and Crumbles: direct JSON health endpoints must return successful
  HTTP responses and `status: healthy` or `status: ok`. Transport/HTTP failure is
  Error; an unexpected status is Warning. Sources are `report-lellostore` at
  `http://lellostore:8080` (`/health`) and `report-crumbles` at
  `http://crumbles:8080` (`/api/health`), with 8-second timeout and 16-KiB limit.
- Backups: missing result/time warns, explicit failure is Error, and last successful
  backup older than 48 hours warns. The 48-hour threshold is provisional pending
  confirmation of the backup freshness policy.
- TLS: validity failure is Error; expiry under 30 days warns and under 7 days is
  Error. Missing certificate probes warn. Scope: auth, pezzottify, pezzottflix, ai,
  agents, store and crumbles under `lelloman.com`. Agents uses internal TLS, so
  probes must use the appropriate trust chain, not disable verification.

## Coverage gaps

**No internet, backup or TLS probe collector was deployed by this update.** These
items intentionally warn until their evidence is connected; the report does not
invent successful results. At verification, Pezzottify's 15-minute HTTP error ratio
and Simple Agents' exhausted-recovery metric were also unavailable.

The `extras` query reserves the following metric contract for future collectors:

- `talia_report_probe_success{host="Homelab|VPS-EU|VPS-US",kind="dns|https"}`
- `talia_report_backup_success` and `talia_report_backup_last_success_timestamp_seconds`
- `talia_report_tls_valid{domain="..."}` and
  `talia_report_tls_expiry_timestamp_seconds{domain="..."}`

These names are not currently populated. Probe samples older than 120 seconds are
excluded. Collectors must stop exposing stale success after collection failure;
backup completion/expiry values are Unix timestamps, while success/validity is 0/1.

## Conditional assessment

`facts` computes states and measured problems. `issues` selects only anomalous
items. The analysis condition `ctx => ctx.steps.issues.value.items.length > 0`
skips inference altogether when every item is healthy. Otherwise the configured
**code:smart** model receives only the anomaly evidence and returns a JSON list
keyed by exact item ID. Each assessment appears under that item; there is no
separate generic interpretation. AI prose cannot change deterministic severity.

Invalid JSON, missing item assessments or inference failure preserves measured
findings and shows an assessment-unavailable note. Unknown/healthy item IDs are
never rendered. Prompts treat input text as evidence, not instructions. The
assessment is advisory and cannot establish a cause that the checks did not measure.

All collection steps are optional so a source failure can produce an explicit
item result. Transformation/composition remain required; their failure fails the
run. Automatic engine-failure notifications are not supported. The deadline stays
20 minutes, including model wake/loading; the renderer appends period and run ID.

## Reproduce and edit

The destination is redacted as `telegram-CHAT_ID` in Git. Read current definitions
before saving, preserve later edits, use the current version as `expected`, increment
version, and use stable request IDs for retries. Provision the two HTTP sources
above alongside `homelab-prometheus` before importing. The JS source files
[checks](report-checklist-facts.js) and [composer](report-checklist-compose.js) are
mirrored into all three JSON definitions.

This version requires engine support for analysis `when`. False records `skipped`
without an AI run. Existing definitions without a condition remain unconditional.
New installations should save disabled, preview with `send:false`, inspect the
result, then enable. Sending a preview is a separate explicitly requested operation.

## Verification — 2026-09-25

- All 148 engine library tests passed, including conditional skip with zero HTTP
  inference requests, selected input isolation, invalid-condition handling and
  completed-inference reuse after restart.
- `node deploy/test-host-reports.cjs` covers all 13 items, healthy compact output,
  thresholds, missing coverage, severity precedence, host aggregation, per-item
  assessment placement and malformed AI output fallback.
- Unsent live preview `report-38be259e5ded107a17f041d4ffab4ef0` completed all 14
  steps using **code:smart**. Six items were healthy; seven warned about missing
  evidence. Each warning received its own assessment. No Telegram preview sent.
- Engine image: `registry.homelab:5000/talia:checklist-20260925`, registry digest
  `sha256:84af8ff9d7bd3c5d91c0c9fe210b0a4a96b91db6be765749a907ed932df5a905`.
  Databases were backed up before restart as `*.before-checklist-20260925.sqlite3`
  in the persistent Talìa data directory. The image was built from the tested local
  working tree; it is not identified as a committed Git revision.
