# Scheduled reporting workflows

[TALIA-68](https://crumbles.lelloman.com/w/LLPR/TALIA/68) introduces server-owned
reports: collect data, execute checks/scripts, run Simple Agents, compose a
retained report, and send it as HTML plus plain-text email. No dashboard needs to
be open. Simple Agents is a supported workflow step in this increment; general
agent-driven alert investigations and Crumbles integration remain separate work.

## Definitions and steps

Report definitions are versioned and editable through MCP without a service
restart. A definition contains `id`, `version`, `enabled`, `schedule`, `steps`,
`compose` and named email `destinations`. Optional `period_ms` defaults to 24 hours;
`timeout_ms` defaults to one hour. A run pins the entire definition and its
reporting period. Definition changes affect future runs only.

Steps execute in order. Later steps can inspect earlier outcomes as
`ctx.steps.STEP_ID = {status,value,error}`. `optional:true` records failure and
continues; the default stops the report. Optional failures are always appended to
the final email, even if the composer omits them. A mandatory Simple Agents step
must succeed before composition. Supported steps:

| Kind | Definition | Retained output |
| --- | --- | --- |
| `read` | `variable`: existing engine Variable ID | Current Instance, including wire `value`, timestamp, quality and revision |
| `source` | `source`: existing DataSource ID; `request`: JS function returning a SourceRequest | `{wire: ...}` from Prometheus query/range or HTTP GET/HEAD |
| `script` | `source`: JS function | JSON result |
| `simple_agents` | `provider`, `instructions`, `inputs`: previous step IDs | Session ID, terminal state, typed result and SHA-256 of the result document |

Request and transformation functions receive `ctx.now` (run creation time in UTC
milliseconds), `ctx.period.start/end`, `ctx.report`, `ctx.run`, `ctx.steps`, and
`ctx.decode(wire)`. Source requests use the existing bounded/authenticated source
adapters; URLs and credentials stay in DataSource configuration. Mutating HTTP
methods are rejected. Scripts are synchronous bounded QuickJS functions without
network, filesystem or process globals. I/O is performed by workflow steps. Values
retain Talìa's tagged wire format until explicitly decoded; compose special values
with `String(...)` if their display is needed.

`compose(ctx)` returns `{subject,summary,sections:[{title,text}]}`. Talìa builds a
formatted email and escapes all supplied text, including LLM output. Raw HTML,
remote images and scripts are not accepted from the model. Both HTML and plain
text are retained with the run. Agent results include artifact IDs, but this
version consumes the result summary rather than downloading report artifacts.

The [morning homelab example](../reports/examples/morning-homelab.json) queries
Prometheus, computes observations, asks Simple Agents for analysis, and composes
an email. Replace `prometheus`, `observer` and `my-email` with configured names.
It starts with scheduling disabled so it can be previewed first.

## MCP operations

All `reports_*` operations require current global authoring/admin authority.
Viewer dashboard grants do not expose reports or arbitrary source data. Report
metadata and results can contain operational data and should be treated accordingly.

- `reports_save`: `{definition,expected,requestId}`; expected zero creates.
- `reports_list` / `reports_get`: discover and read definitions.
- `reports_run`: `{id,send,requestId}` returns a durable `run_id` immediately.
- `reports_run_get`: `{id:run_id}` returns step outcomes, current agent session,
  frozen definition, report HTML/text and per-destination delivery status.
- `reports_runs`: `{report,limit?,before?}` lists bounded summaries; use the
  returned `next_before` as the exclusive run-ID cursor.
- `reports_deliver`: `{id:run_id,requestId}` sends a completed, unsent preview
  without rerunning its checks or agent. A second delivery request is rejected.
- `reports_prune`: `{before:UTC_milliseconds,requestId}` removes terminal runs
  older than the cutoff; request tombstones remain to prevent duplicate execution.

Use a new request ID for a new operation and the same ID/body for retries.
`send:false` suppresses email only: it still runs checks and Simple Agents and
therefore can consume agent resources. Preview via `reports_run_get`; delivering
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
interrupted before that commit may run again. Before contacting Simple Agents,
Talìa persists the exact request, idempotency key, provider origin and caller ID.
After restart or an ambiguous submission response it resolves that same key,
replaying only the identical submission if absent. It never invents a replacement
key. Provider identity changes fail explicitly; rotating its token is supported.
The pinned public client validates session bindings, bounds responses and disables
redirects. Nonterminal sessions are polled every five seconds; terminal state and
result must agree, and uncertain external-effect receipts do not become success.
If an agent step fails after preparation, its failed output retains the exact
submission and last known session details for reconciliation, without credentials.

The Talìa deadline ends the reporting workflow. It does not claim to cancel a
remote agent: the retained session/key remains available for operator reconciliation
and the agent also has its own server-enforced budget. Waiting/paused/input states
remain visible until completion or deadline; Talìa does not approve requests or
answer questions automatically. Provider capabilities are restricted to
observational work for this increment.

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

Set `TALIA_REPORT_AGENTS=/run/talia/report-agents.json` to a private JSON map:

```json
{
  "observer": {
    "origin": "https://agents.example.com",
    "caller_id": "talia",
    "token_file": "/run/talia/simple-agents.token",
    "profile_id": "read-only-observer",
    "capabilities": [],
    "binding_ids": [],
    "budget": {
      "wall_time_secs": 300,
      "cpu_time_secs": 240,
      "memory_bytes": 536870912,
      "max_processes": 64,
      "workspace_bytes": 67108864,
      "evidence_bytes": 16777216,
      "retention_secs": 604800
    }
  }
}
```

Provision the caller, allowed profile, capability bindings and token in Simple
Agents first. The example with empty capabilities analyzes supplied data; adding
observational capabilities/bindings requires matching Simple Agents configuration.
`external_mutation`, `repository_write` and `human_input` are rejected. Credentials,
profile and budgets are operator-owned rather than report-script arguments.
HTTPS is required outside numeric loopback fixtures. Keep credentials readable
only by the service owner/container UID 65532. Provider contents and token files
are read asynchronously during work, so atomic replacements require no restart.
Adding the environment setting/mount initially requires container recreation.

Email uses the existing private `TALIA_ALERT_PROVIDERS` file; see
[SMTP provider configuration](alerts.md#provider-configuration). No new provider
credentials or recipient addresses are committed in this feature.

The public client/protocol source bundle is vendored unmodified under
`engine/vendor/simple-agents-client`, from clean Simple Agents revision
`6b520b0ae2bd79ddfb72231c8a8f17766733173f`. It includes license, source provenance,
contract examples and SHA-256 inventory; Talìa builds without a sibling checkout.
The original archive SHA-256 is
`98a198be38b2b5feb8b768cc125523ba142eeebb6062dc94b6aa5e8199f8b971`.

SQLite migration 13 adds report definitions, runs and request tombstones. Back up
the engine database before upgrading. Older binaries reject this schema; do not
roll back by restoring stale operational state.

## Qualification

`cargo test --manifest-path engine/Cargo.toml --offline` covers pinned runs,
version conflicts, request replay, viewer denial, scheduling overlap, safe HTML,
script budget/optional failure, actual HTTP collection and SMTP multipart delivery,
unknown-send restart recovery, and an ambiguous Simple Agents submission recovered
by key after reopening SQLite (one submission, retained result used in composition).
`node dashboard/tests/access.mjs` verifies discoverable report tools, authoring,
background preview execution and retained HTML through the official HTTP MCP SDK
against the actual HTTPS/OIDC service. This is fixture evidence; no live agent run
or email to a real recipient is sent by these tests.
