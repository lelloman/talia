# Structured homelab Telegram reports

[The definitions](homelab-telegram-reports.json) record the approved daily reports
at **09:00, 14:00 and 19:00 Europe/Rome**, including daylight-saving changes.
They were configured through the running service's operator MCP on 2026-09-23;
no deployment or restart is needed to edit them.

Each report reads `homelab-summary`, queries `homelab-prometheus` for rolling
24-hour scrape availability, transforms the results, and requests a short
interpretation from the configured simple-ai account. The layout uses status,
attention, host, disk, services, interpretation and data sections with short
metric bullets. Internal-state and retention metadata are excluded.

Attention thresholds are snapshot age above 120 seconds, non-good quality,
unreachable/unknown targets, missing readings, disk free below 10%, CPU or memory
at least 90%, and scrape availability below 99%. These are report presentation
rules, not alert policies. Scrape reachability does not prove application health.
Application events and Talìa alert records are outside this report's coverage.

The deadline is twenty minutes, including GPU wake-up and model loading.
The AI HTTP request uses the remaining report deadline instead of a separate
two-minute cutoff. AI analysis is optional: failure leaves the measured
facts available and is explicitly shown in the report. Collection and composition
are required. All reports use a rolling 24-hour window, not time since the previous
report. The standard renderer adds the period and durable run ID as a footer.

## Reproduce or update

The private destination ID is replaced with `telegram-CHAT_ID` in Git. Replace it
with an approved delivery destination from Settings → Telegram. Credentials stay
in the service; none are included here.

Read `reports_list`/`reports_get` first. The saved versions (3, 2, 2) describe the
deployment at capture time, not universal import versions. For each definition,
use `reports_save` with `expected` equal to its current version and set the new
definition version to `expected + 1`; use expected zero/version one for a new ID.
Supply a fresh stable request ID for each operation and preserve later edits.

The captured definitions are enabled. For a new installation, first save them
disabled, run a preview with `reports_run` and `send:false`, inspect its result
with `reports_run_get`, and explicitly deliver the accepted preview with
`reports_deliver`. Then enable the three schedules with versioned saves.

## Live verification

The structured preview `report-a7a1374f61ff6a01fc8b7e9b56714dc3` completed with
successful collection, transformation, AI analysis and composition. Telegram
accepted its single message part and the report's destination status was `sent`.
All three daily schedules were enabled. This confirms Telegram acceptance, not
a read receipt or an already-observed future scheduled execution.
