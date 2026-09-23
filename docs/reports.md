# Scheduled reporting workflows

Server-owned reports collect data, execute checks/scripts, compose a retained
summary and send HTML plus plain-text email or Telegram messages. No dashboard
needs to be open. LLM analysis through a Talìa-owned harness using simple-ai
completions is planned separately; it is not currently available.

## Definitions and steps

Report definitions are versioned and editable through MCP without a service
restart. A definition contains `id`, `version`, `enabled`, `schedule`, `steps`,
`compose` and named email/Telegram `destinations`. Optional `period_ms` defaults to 24 hours;
`timeout_ms` defaults to one hour. A run pins the entire definition and its
reporting period. Definition changes affect future runs only.

Steps execute in order. Later steps can inspect earlier outcomes as
`ctx.steps.STEP_ID = {status,value,error}`. `optional:true` records failure and
continues; the default stops the report. Optional failures are always appended to
the final email, even if the composer omits them. Supported steps:

| Kind | Definition | Retained output |
| --- | --- | --- |
| `read` | `variable`: existing engine Variable ID | Current Instance, including wire `value`, timestamp, quality and revision |
| `source` | `source`: existing DataSource ID; `request`: JS function returning a SourceRequest | `{wire: ...}` from Prometheus query/range or HTTP GET/HEAD |
| `script` | `source`: JS function | JSON result |

Request and transformation functions receive `ctx.now` (run creation time in UTC
milliseconds), `ctx.period.start/end`, `ctx.report`, `ctx.run`, `ctx.steps`, and
`ctx.decode(wire)`. Source requests use the existing bounded/authenticated source
adapters; URLs and credentials stay in DataSource configuration. Mutating HTTP
methods are rejected. Scripts are synchronous bounded QuickJS functions without
network, filesystem or process globals. I/O is performed by workflow steps. Values
retain Talìa's tagged wire format until explicitly decoded; compose special values
with `String(...)` if their display is needed.

`compose(ctx)` returns `{subject,summary,sections:[{title,text}]}`. Talìa builds a
formatted email and escapes all supplied text, including source output. Raw HTML,
remote images and scripts are not accepted from scripts. Both HTML and plain
text are retained with the run.

The [morning homelab example](../reports/examples/morning-homelab.json) queries
Prometheus, computes observations, and composes an email. Replace `prometheus`
and `my-email` with configured names.
It starts with scheduling disabled so it can be previewed first.

## MCP operations

All `reports_*` operations require current global authoring/admin authority.
Viewer dashboard grants do not expose reports or arbitrary source data. Report
metadata and results can contain operational data and should be treated accordingly.

- `reports_save`: `{definition,expected,requestId}`; expected zero creates.
- `reports_list` / `reports_get`: discover and read definitions.
- `reports_run`: `{id,send,requestId}` returns a durable `run_id` immediately.
- `reports_run_get`: `{id:run_id}` returns step outcomes,
  frozen definition, report HTML/text and per-destination delivery status.
- `reports_runs`: `{report,limit?,before?}` lists bounded summaries; use the
  returned `next_before` as the exclusive run-ID cursor.
- `reports_deliver`: `{id:run_id,requestId}` delivers a completed, unsent preview
  without rerunning its checks. A second delivery request is rejected.
- `reports_prune`: `{before:UTC_milliseconds,requestId}` removes terminal runs
  older than the cutoff; request tombstones remain to prevent duplicate execution.

Use a new request ID for a new operation and the same ID/body for retries.
`send:false` suppresses delivery only: it still runs all collection and script steps. Preview via `reports_run_get`; delivering
that preview is an explicit separate operation. Configure an existing enabled
`email` destination with `alerts_destination_save`; reports reuse its SMTP provider.

## Schedules, restart and failure behavior

Schedules reuse Talìa's interval or daily calendar rules, including IANA time zones
and optional ISO weekdays. Minimum report interval is one minute. Calendar DST
handling is shared with monitoring: first occurrence of repeated local time,
first valid local minute for a missing time. Report periods are elapsed durations;
24 hours is not necessarily the preceding local calendar day around DST.

Only one collection/composition execution per definition runs at a time. Overlap
is skipped; missed schedules coalesce to one current run. Scheduling then moves
forward without replaying every missed morning. Unchanged schedules retain their
next due time across definition edits. Disabling a definition stops future
scheduled admissions; it does not undo an admitted run.

Each step result commits before the next step begins. Read-only checks/pure scripts
interrupted before that commit may run again. The deadline ends local report
execution; it does not undo completed checks or accepted deliveries.

Email is tracked independently per destination. Disabled or changed destinations
are refused before dispatch. Explicit transient SMTP rejection gets up to three
attempts, 30 seconds apart and within the deadline. A lost response, timeout or
restart during sending produces `unknown` and is not automatically resent.
Vendor acceptance means `sent`, not read. Completed runs with optional failures or
unsuccessful/unknown delivery are `partial`; required-step/composition failure is
`failed`. Neither status is silently represented as a successful report. Failed
reports and delivery outcomes are inspectable through MCP; automatic report-failure
alerts and a dedicated web report-history page are not included yet.

Bounds: 100 definitions, 16 steps, 16 distinct destinations, 32 KiB per successful
step or composed JSON output, 128 KiB per definition, 1 MiB per run record, and 10,000
retained runs. Four asynchronous advances can be in flight; no network wait holds
a store borrow or SQLite transaction. Use prune for terminal retention; active
work and idempotency tombstones are not pruned.

## Operator configuration

Email uses the existing private `TALIA_ALERT_PROVIDERS` file; see
[SMTP provider configuration](alerts.md#provider-configuration). No new provider
credentials or recipient addresses are committed in this feature.

## Upgrade from the retired agent integration

Migration 15 removes Simple Agents execution and its credentials. Definitions
containing `simple_agents` steps are disabled and their versions incremented.
These steps become non-executable `unavailable` records with the original step
retained for inspection. Saving or running such a definition fails until an admin
replaces those steps; nothing silently skips an analysis requirement.

Affected queued/running reports fail explicitly. Completed reports and already
composed deliveries remain available; original steps remain under `original` and pending session
evidence remains in the run as `retired_execution`. Talìa does not contact or cancel
external sessions. If one was running before the upgrade, reconcile it separately.
The old provider environment variables/files are no longer read and can be removed
from the deployment. Back up SQLite before upgrading; older binaries reject
schema 15. Normal read/source/script reports continue to work.

## Qualification

`cargo test --manifest-path engine/Cargo.toml --offline` covers pinned runs,
version conflicts, request replay, viewer denial, scheduling overlap, safe HTML,
script budget/optional failure, actual HTTP collection and SMTP multipart delivery,
unknown-send restart recovery, and migration of retired workflows while preserving
completed reports, pending deliveries and historical evidence.
`node dashboard/tests/access.mjs` verifies discoverable report tools, authoring,
background preview execution and retained HTML through the official HTTP MCP SDK
against the actual HTTPS/OIDC service. This is fixture evidence; no live email to a real recipient is sent by these tests.

## Telegram destinations

Use Settings → Telegram to pair a destination, then reference its `telegram-CHAT_ID` in a report. Managed Telegram delivery is tracked per message part; each part retains its report reference. See [Telegram](telegram.md).
