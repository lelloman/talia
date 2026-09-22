# Product specification

Accepted P3 collection, scheduling and Watch decisions are consolidated in
[Monitoring contract](monitoring.md), superseding corresponding open questions here.
Accepted P4 authoring and live-control decisions are specified in the
[MCP contract](mcp-contract.md); it defines implementation requirements, not completed tools.

Status: high-level direction recorded from discussion, 2026-09-18.
Detailed behavior and architecture are not signed off. This document supersedes
the earlier speculative v0.1 design; its numerical defaults and implementation
choices are not requirements.

Planning, open-decision tracking and execution status live in the
[Crumbles Stories](implementation-plan.md). This document is technical reference;
its open questions are inputs to Story refinement.

The [user access contract](user-access.md) defines admin-only authoring/sharing,
read-only dashboard-scoped viewer access, account defaults and client overrides.
It supersedes earlier open questions about dashboard ownership and user permissions.

## 1. Dashboards for humans

Talìa is LLM-driven: users ask agents to define and modify dashboards through
MCP, rather than directly editing dashboard layouts. Humans view and interact
with the resulting interfaces. Checks, schedules, alerts and outcome triggers
are also configurable through MCP.

A dashboard defines a small application, including interactive controls and
behavior, rather than only charts. Each web or native Android client instance
selects its own dashboard configuration. The same definition is usable on both
platforms; per-client configuration does not mean separate platform languages.

The hierarchy is **Dashboard → Screen → ViewGroup → View**. Multiple screens can
share a surface, such as a navigation screen beside or below a content screen.
Navigation and composition are configurable per client. ViewGroups contain
Views and nested ViewGroups. Views include buttons, sliders and switches as well
as data visualisations.

UI authoring uses restricted JSX-like syntax compiled into a validated shared
UI tree. Separate JavaScript ViewModels define state and behavior. Both use a
stable engine contract, with read, write and subscribe operations. See the
[dashboard model](dashboards.md) for the agreed boundaries and open details.

MCP distinguishes persistent dashboard authoring from manipulation of a specific
live dashboard's ViewModel. Live manipulation cannot change View definitions;
ViewModel modifications mark the instance dirty and are discarded on reload.
Reload restores the saved definition, but does not undo engine-side effects.

Dashboards combine:

- Current values and historical time series queried from Prometheus.
- Results from Talìa's own probes and checks.
- Status and outcomes of Simple Agents sessions and Crumbles tickets tracked
  by Talìa.

Prometheus remains responsible for scraping services and storing metrics. Talìa
consumes that data for dashboards, checks, anomaly evaluation, and investigation
context. How much queried data Talìa caches or retains is undecided.

“Grafana-like” describes flexible dashboard presentation, not a requirement for
a human-facing layout editor. Full Grafana
feature parity, dashboard import compatibility, and retaining or retiring the
existing Grafana deployment have not been agreed.

## 2. Periodic checks and tasks

Talìa schedules monitoring work and tracks its execution and results. Work can
use any of these mechanisms:

| Mechanism | Purpose |
|---|---|
| Direct check | Query Prometheus, hit an endpoint, or perform a Talìa probe |
| Simple Agents session | Execute structured LLM-assisted checks, analysis, and investigations, including tool-using work |
| Crumbles ticket | Delegate work that benefits from a ticket and its workflow |

Talìa-managed LLM-assisted monitoring execution goes through Simple Agents. Talìa does not call
SimpleAI directly or implement its own LLM tool loop. Simple Agents sessions
can be submitted directly without creating a Crumbles ticket first.

Delegation includes lifecycle tracking, not just submission. Talìa follows
Simple Agents sessions and Crumbles tickets through progress and completion,
consumes their results, and supports configurable outcome triggers. Crumbles
integration remains part of the product scope, with implementation scheduled after
homelab migration. Alert delivery is implemented first.

Illustrative triggers include notifying on a failed outcome, starting a
follow-up investigation, or issuing another check or ticket based on a result.
The precise ticket states, result format, trigger predicates, and treatment of
blocked, overdue, cancelled, or reopened work still need definition.

## 3. Anomaly detection and alerts

Users have agents create and configure alert rules through MCP. A dedicated
human-facing rule editor is not part of the agreed authoring model.
Rules can evaluate Prometheus metrics, probe/check results, and delegated task
status or outcomes.

Configuration includes the condition, how long it must persist, and actions
when it fires or recovers. Available action categories are:

- Present alert state to dashboards and optionally deliver Android push, browser Web Push, email,
  or Telegram notifications using staged, reusable response policies.
- Start a structured task in Simple Agents (post-migration integration).
- Issue a Crumbles ticket and track its lifecycle and outcome (post-migration integration).

Alerts and outcome triggers connect monitoring to follow-up work. For example,
a metric condition can start an investigation, whose result can trigger a
notification or a ticket. This is an illustration of composition, not a
prescribed default workflow.

Anomaly detection does not imply that every check uses an LLM. Deterministic
rules and LLM-assisted analysis are both part of the scope. Specific detection
methods, rule syntax, notification channels, and execution policies remain open.

## Runtime server engine

The server owns **DataSources, Pipelines, Variables and Watches**. A DataSource
specifies a connection, a Pipeline collects/transforms data, a Variable exposes
a named typed result, and a Watch observes Variables with its own persisted state
and actions. Pipelines run automatically, by trigger, or on explicit request.
Stateful Watches can remember earlier conditions before invoking later actions.

Definitions can be created, updated or removed at runtime without service
restart, with validation before activation. Persist definitions and retained
engine data/state, including Watch state; restored values preserve their age.
SQLite is a candidate. Monitoring runs independently of connected dashboards.
The server uses Rust/Axum with an embedded JavaScript runtime for configurable
Pipeline and Watch logic. Runtime selection and detailed APIs remain open.

Clients have an API/SDK for server read, write and subscribe operations, plus
their local UI and JavaScript ViewModel runtime. Client subscription reactions
are not persistent server Watches.

Watch instances reference reusable definitions with their own input/source
bindings, thresholds and state. Definition changes affect all references;
instance parameter changes affect only that instance. This model also applies
to UI elements, ViewModel elements and functions. Update activation and state
migration details are still open. See the [engine model](engine.md).

### Variable behavior

Variables have a declared type, latest value, timestamp and quality status, with
configurable history. Computed values have a getter, optional setter, their own
persistent server-side internal state, and an injected time provider. They can
query sources, derive values, cache results or maintain other computation state.
Lazy behavior is covered by this model rather than a separate kind of Variable.

Computed values declare dependencies for re-evaluation while subscribed, and
support explicit refresh/invalidation for external sources. Getters, setters and
reads may await without blocking other operations on the same instance. Only short
synchronous state updates are atomic; whole async operations are not. Computed
values configure shared in-flight refresh or independent reads, with setters and
other operations still able to proceed during shared refresh.

Dependency cycles fail immediately. Cancelled work is skipped before it starts;
running cancellation prevents subsequent publication without holding the instance
while I/O finishes. Effects already dispatched are not undone. These guarantees do
not promise multi-Variable transactions, rollback or atomic external effects. See
[Variable semantics](engine.md#variables-and-computed-values) for conflict handling,
shared-reader cancellation and other open details.

## Responsibilities

| System | Responsibility in the discussed product direction |
|---|---|
| Talìa | Dashboard and alert configuration, Prometheus consumption, probes, scheduling, delegated-work tracking, outcome triggers, alerts |
| Prometheus | Metrics scraping, storage, and queries consumed by Talìa |
| Simple Agents | Structured execution of all LLM-assisted checks, analysis, and investigations |
| Crumbles | Tickets and workflows whose progress and outcomes Talìa follows |

SimpleAI may be used internally by other systems; that creates no direct Talìa
integration. Existing homelab use of Grafana, Loki and Alertmanager is background
context, not a decision to make those services required Talìa dependencies.

## Scope examples for later acceptance criteria

These examples describe the discussed capabilities, without fixing their
implementation or release sequence:

1. An agent defines and saves a dashboard containing a Prometheus chart, an
   endpoint check result, controls, and delegated monitoring work status. The
   shared UI and JavaScript logic run on both web and native Android clients.
2. A scheduled endpoint check records its result for dashboard and rule use.
3. A scheduled Simple Agents task analyses monitoring data; Talìa follows the
   session and makes its result available to an outcome trigger.
4. A scheduled operation issues a Crumbles ticket; Talìa follows it to completion
   and executes the configured action for its outcome.
5. An agent defines an alert through MCP with a persistence condition and
   separate firing/recovery actions, including notification or delegated work.
6. An agent manipulates a live ViewModel through MCP. The instance becomes dirty,
   its View definitions remain unchanged, and reload discards the temporary
   changes while preserving any effects already performed through the engine.

7. An agent adds a DataSource, collection Pipeline, derived Variable and Watch
   while the server runs, without restarting it. Monitoring continues without
   connected clients and recovers persisted Watch state after a restart.
8. Two Watch instances reference shared logic with separate thresholds/state.
   Editing one instance leaves the other untouched; changing the definition
   updates both references. Equivalent reuse applies to UI/ViewModel definitions
   and functions, subject to still-open activation and migration semantics.

## Questions for detailed specification

- Engine types and APIs, definition activation, Watch evaluation/recovery, and
  shared-definition state migration; see [engine questions](engine.md#open-details).
- Exact UI component vocabulary, layout rules, binding expressions, responsive
  behavior, ViewModel runtime, dashboard sharing and permissions.
- MCP discovery/editing tools and whether the user talks to a built-in assistant,
  an external MCP agent, or both.
- Live instance targeting, dirty-state details, reload/version semantics and
  concurrent authoring; see [dashboard questions](dashboards.md#open-details).
- Probe types, MCP schedule configuration, time zones, manual runs, overlapping work,
  missed schedules, retries, and deadlines.
- Crumbles lifecycle mapping and result contract; how results are tested by
  triggers, including reopening and changed outcomes.
- Simple Agents task templates, tool permissions, budgets, human input, and
  result contracts.
- Alert evaluation and anomaly methods; missing/stale data behavior, severity,
  grouping, acknowledgements, suppression, repeat and recovery behavior.
- Notification channels, action configuration, and limits against duplicate or
  recursively triggered work.
- History and retention, Prometheus query caching, access control, and deployment.
- How existing homelab checks, dashboards, alerts and consumers transition to
  Talìa, including the future roles of Grafana, Loki and Alertmanager.

These questions are not resolved by the previous draft's proposed defaults.

## First implementation milestone

Both web and native Android are required in the first milestone. Features are
built against one shared dashboard/UI/logic contract and exercised on both
clients as they are introduced. The [implementation plan](implementation-plan.md)
drafts the end-to-end slice, runtime prototypes and validation gates. Its proposed
technical choices remain subject to prototype evidence; the detailed product
questions above are not silently resolved by the plan.

## Alert delivery contract

See [configurable alerts](alerts.md) for the accepted staged-policy, acknowledgement,
silence, destination and durable delivery model. Android push, browser Web Push, email and Telegram
alert delivery precede deployment and homelab migration; Simple Agents and Crumbles
delegation follow migration. The original alert workstream is tracked in TALIA-47; browser destinations in TALIA-66.

## Web monitoring display

The web client can show its dashboard as a fullscreen monitoring surface, hiding
application navigation while keeping connection messages, dashboard diagnostics,
restart controls and update/dirty indicators visible. Fullscreen entry/exit is a
presentation change and does not replace the live dashboard. If the browser
rejects native fullscreen, an escapable in-window monitoring surface is used.

Each browser/account can save an ordered playlist of accessible dashboards, a
whole-second dwell time per dashboard (5–3600 seconds), and whether rotation starts
on entering monitoring mode. Previous/next and play/pause controls are available;
pointer movement, touch or keyboard interaction reveals the controls. Escape or
Exit returns to the normal shell. Fullscreen requires an explicit gesture after
page reload; saved settings never force a browser into fullscreen.

This first implementation cycles whole dashboards on the web. It uses the normal
authorized client-selection path, with one live instance at a time. Switching
loads the target's saved definition and fresh ViewModel; returning to a dashboard
does not restore its earlier transient state. Server effects survive. Automatic
rotation pauses on dashboard errors or dirty live edits, and manual switching
also refuses to discard dirty edits. Hidden or disconnected clients suspend the
timer and resume with a full dwell period, without catching up missed rotations.
Removed access pauses the current display and inaccessible playlist entries are
skipped. Per-Screen rotation and native Android presentation controls are future
increments, not included in TALIA-67.
