# Existing systems and migration considerations

Status: historical source inventory and refinement input, not an approved rollout plan.
Current migration planning, decisions and cutover acceptance belong in
[LLPR/TALIA-9](https://crumbles.lelloman.com/w/LLPR/TALIA/9).
The [product specification](specification.md) records the current scope.

Inspected 2026-09-17 from local source; no live infrastructure was queried.
Repository revisions:

- Simple Agents: `870f4777441bdf414d0b7e4966f867938b00a6c4`
- Homelab: `2dc1d1c799469a1adc9c274117d8c86dccd568cd`

Source links below assume sibling checkouts. Production state may differ.

## Evidence and implications

| Source | Finding and implication |
|---|---|
| [Simple Agents README](../../simple-agents/README.md) | The planned monitoring client owns observations, schedules, dashboards, assessments and warnings; this is Talìa's starting boundary |
| [Session contract](../../simple-agents/contracts/v1/README.md), [observer example](../../simple-agents/contracts/v1/examples/observe.json), [client](../../simple-agents/docs/CLIENT.md) | Repository-free observation, bounded sessions, durable replay and caller-scoped idempotency already exist |
| [Metrics](../../simple-agents/docs/metrics.md) | Collection credentials are separate from session callers; Runner-loss metrics account for managed idle hosts |
| [Homelab README](../../homelab/README.md) | Prometheus/Loki/Grafana/Alertmanager already exist; public access passes through VPS and WireGuard; README records a Promtail Docker-discovery incompatibility |
| [Health checker](../../homelab/health-monitor/health-check.py) | Hourly SQLite snapshots, deterministic checks and direct Telegram notifications; replace with Talìa collection, storage and finding lifecycle |
| [Investigation launcher](../../homelab/health-monitor/investigate.py) | Builds health context and launches local Codex; replace with durable Simple Agents assessments |
| [Homelab Agent](../../homelab/homelab-agent/README.md), [health tools](../../homelab/homelab-agent/src/homelab_agent/tools/health.py) | CLI/Telegram assistant reads health.db and refreshes the script with suppressed alerts; migrate these tools before retiring the writer |
| [Prometheus config](../../homelab/monitoring/prometheus.yml), [rules](../../homelab/monitoring/alerts.yml) | Preserve existing scrape/alert coverage and timing, including storage and application alerts |
| [Alertmanager entrypoint](../../homelab/monitoring/alertmanager-entrypoint.sh) | Routes to Telegram and supports an optional additional webhook; qualify authenticated Talìa ingestion without disrupting existing delivery |
| [Runbook](../../homelab/RUNBOOK.md) | Deployment, incident response, shutdown/startup and service onboarding reference health-check.py; update all consumers at cutover |

The health-monitor README differs from code: the launcher uses Codex, TLS uses
OpenSSL against three named domains, and most database checks open read-only and
execute `SELECT 1` rather than checking integrity. Simple Agents has a special
container-local `PRAGMA quick_check`. The code also sends change/new-problem
Telegram notifications, beyond the README's snapshot description.

Alertmanager failures can return an empty alert list; TLS failures can omit
domains entirely. Similar collector failures can therefore disappear from the
aggregate. Container Docker health and scrape-target results are not fully used
by the overall-status calculation. Talìa must preserve check coverage, not
replicate these failure semantics. Treat the reported Promtail issue as a gap
requiring verification and remediation, never as evidence of zero log errors.

## Initial inventory seed

The script expects 16 containers: simple-agents, caddy, wg-easy, prometheus,
grafana, alertmanager, telegram-bot, loki, promtail, node-exporter,
pezzottify-server, pezzottify-downloader, lelloauth, lellostore, pezzottflix and
favzetto. Import IDs and validate them against the approved deployment inventory
before rollout.

It checks host HTTP endpoints for Prometheus, Alertmanager, Grafana and Loki,
plus container-local endpoints for Simple Agents, the downloader, LelloAuth,
LelloStore, Pezzottflix and Favzetto. Database configuration covers Simple Agents,
LelloAuth, LelloStore, Pezzottify catalog/server, SimpleAI, Crumbles, Pezzottflix
and Favzetto. TLS currently covers pezzottify/auth/store public domains only.

Prometheus additionally includes jobs for SimpleAI, idle-manager, observo, Knot
Resolver, dns-collector and knot-observer. Reconcile these sets explicitly;
neither the health script nor scrape configuration alone defines all services.
Add public ingress, VPN/DNS and expected mount checks according to deployment
requirements; do not claim those paths are covered by container liveness alone.

## Engine implications of the current direction

As of 2026-09-18, the [server engine](engine.md) is Rust/Axum with embedded JS
Pipeline and Watch logic. Migration must map connections to DataSources,
collection/transformation work to Pipelines, results to Variables, and stateful
conditions/actions to Watches. The server owns these independently of clients.
Definitions and retained data/state must persist, and monitoring configuration
changes must not require restarting Talìa. SQLite remains a candidate.

Variable migration must preserve value timestamps and quality, choose history
policies, and include computed-value internal state in server recovery. Cached
results must not become fresh observations merely because Talìa restarted.

Shared definitions are referenced with per-instance parameters and state, rather
than copied. Migration planning must account for Watch state and shared-definition
updates. The historical source inventory above is unchanged by these decisions.

## Historical transition questions for Story refinement

Talìa is intended to take over homelab health monitoring and provide a configurable
dashboard and alert experience authored by agents through MCP. The following work needs planning after the
detailed specification is discussed:

- Inventory existing Grafana dashboards and identify the panels, queries and
  interactions needed in Talìa. Decide whether migration is manual or supported
  by an import mechanism; Grafana compatibility is not currently promised.
  The target is a shared JSX-like UI definition plus JavaScript ViewModel,
  rendered on web and Android, not a port of Grafana's editing interface.
- Plan per-client dashboard assignment and configurable navigation/composition.
  Validate the same definitions on both platforms, including interactions, engine
  subscriptions and the live dirty/reload boundary.
- Keep persistent MCP authoring separate from temporary live ViewModel changes;
  neither live experiments nor a client reload should silently alter saved
  definitions or roll back engine-side effects.
- Map existing health checks to Talìa probes, Prometheus queries, Simple Agents
  tasks or Crumbles workflows. Preserve known coverage while correcting misleading
  missing-data and integrity-check semantics.
- Inventory alert conditions and routes. Decide which evaluation/delivery remains
  in Prometheus/Alertmanager and which moves to Talìa, avoiding duplicate alerts
  or gaps during transition. Keeping Alertmanager permanently is not decided.
- Determine whether Loki is a Talìa data source and verify log collection coverage
  before using absence of errors as evidence.
- Investigate the Crumbles API and define ticket creation, lifecycle tracking,
  result consumption and outcome actions. Crumbles is part of the core scope.
- Define structured Simple Agents tasks, their results, and the permitted tools
  needed for checks, analyses and investigations. No direct SimpleAI integration
  or alternate local LLM execution path is planned.
- Adapt homelab-agent health consumers and operational runbook references before
  retiring health.db updates and the local investigation launcher. Whether the
  entire conversational assistant is replaced remains open.
- Define validation, parallel-running, cutover, rollback, backup and history
  handling before changing production. No rollout duration or retention period
  has been selected.

The earlier staged implementation plan and defaults are superseded. This inventory
is historical evidence from the inspected revisions, not a commitment to retain
every existing component or reproduce its current behavior. Nothing here changes
production or retires an existing service.
