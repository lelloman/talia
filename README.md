<img src="assets/brand/brand.svg" width="80" height="80" alt="Talìa Wide gaze icon" />

# Talìa

Talìa provides agent-authored dashboards, scheduled checks and tasks,
and configurable anomaly alerts. It will also take over homelab health monitoring.

Its three purposes are:

1. **Dashboards for humans.** Agents define interactive interfaces through MCP,
   using Prometheus metrics, probe results, and delegated task status and outcomes.
   Web and native Android clients render the same UI and logic definitions.
2. **Periodic checks and tasks.** Run direct probes or delegate structured work
   to Simple Agents and Crumbles, following that work through completion.
3. **Anomaly detection and alerts.** Agents configure conditions and actions
   through MCP, including notifications and follow-up work.

Prometheus continues collecting and storing metrics; Talìa consumes them.
Simple Agents executes all LLM-assisted work. Crumbles supplies ticket workflows.
Talìa has no direct SimpleAI integration.

Dashboards use restricted JSX-like UI definitions and separate JavaScript
ViewModels, backed by a stable engine interface for read, write and subscribe
operations. Persistent authoring and temporary live ViewModel manipulation are
separate MCP capabilities.

The server engine uses Rust/Axum with an embedded JavaScript runtime for Pipeline
and Watch logic. DataSources, Pipelines, Variables and stateful Watches are
configured at runtime without service restarts. Definitions and retained data/state
are persisted in SQLite. Clients access this server
engine through an API rather than hosting their own monitoring engine.
Variables carry typed values and quality metadata with configurable history.
Computed values support stateful getters/setters, dependency updates and caching;
async operations may interleave on the same instance, with short atomic state
updates and configurable shared-refresh or independent-read behavior.

These documents record the product direction and the accepted contracts refined
for each roadmap stage. Remaining future-stage details are tracked in Crumbles.
Isolated [runtime](docs/runtime-prototype.md) and [client–server transport](docs/transport-prototype.md)
prototypes have been implemented and tested. The shared [P1 dashboard clients](dashboard/README.md)
implement the UI/VM contract on web and native Android. The Rust engine now
collects Prometheus/HTTP data and runs durable Pipelines and Watches. Both clients
can render the same monitoring package. Production deployment remains a later stage.

- [MCP authoring adapter](docs/mcp-authoring.md): authenticated saved authoring, assignment tools and operator setup.
- [MCP engine operations](docs/mcp-engine.md): reads, subscriptions, guarded writes and durable Pipeline actions.
- [MCP live control](docs/mcp-live.md): exact-instance discovery, inspection, temporary execution and guarded reload on both clients.
- [P4 qualification](docs/p4-qualification.md): reproducible MCP, web and emulator acceptance evidence.
- [Users and dashboard sharing](docs/user-access.md): admin/viewer roles, server-enforced dashboard access and account defaults.
- [P4 MCP contract](docs/mcp-contract.md): agent authoring, client identity, permissions and live control; implementation tracked in Crumbles.
- [P3 monitoring qualification](docs/p3-qualification.md): collection, Watches, both clients and current evidence.
- [Monitoring contract](docs/monitoring.md): accepted source, execution, scheduling and recovery rules.
- [P1 dashboard implementation and qualification](docs/p1-qualification.md): client behavior, evidence and remaining acceptance.
- [P0 qualification status](docs/p0-qualification.md): passing four-platform evidence and acceptance scope.
- [Runtime contract](docs/runtime-contract.md): approved execution, authority, lifecycle and failure behavior.
- [Transport findings](docs/transport-prototype.md): real HTTP reads/writes, subscriptions,
  reconnect and action outcomes across all four hosts.
- [Runtime findings](docs/runtime-prototype.md): executable P0 evidence and
  unresolved qualification gaps.
- [Crumbles roadmap](docs/implementation-plan.md): authoritative planning Stories in
  `LLPR/TALIA`; refine each Story into actionable subtasks before implementation.
- [Product specification](docs/specification.md): discussed scope and open
  behavioral questions.
- [Server engine](docs/engine.md): runtime primitives, execution, persistence,
  and reusable definitions with independent instance state.
- [UI/VM contract v1](dashboard/contracts/v1/README.md): shared authoring and rendering semantics.
- [Dashboard model](docs/dashboards.md): shared UI and logic, client composition,
  engine bindings, and persistent versus live MCP operations.
- [Architecture boundaries](docs/architecture.md): integration responsibilities
  and decisions still to make.
- [Existing systems and migration considerations](docs/migration.md): source
  inventory and issues to address when planning the transition.

- [Brand identity](docs/branding.md): selected Wide gaze icon, assets and exploration.

The [P2 durable engine](engine/README.md) provides SQLite-backed state, runtime definition updates, computed evaluation and restart-aware client integration. Its [qualification report](docs/p2-qualification.md) records the P2 baseline; P3 records current integration checks.


The [configurable alert workstream](docs/alerts.md) adds server-owned occurrences,
shared JS response policies, acknowledgement, silences, durable delivery and named
destinations. SMTP email, Telegram, FCM Android push and browser Web Push have provider adapters;
web and native Android expose alert controls independently of dashboard failures.
See [alert qualification](docs/alert-qualification.md) for test evidence and limits.
Live provider credentials, deployment preparation and homelab migration remain next;
General Simple Agents investigations and Crumbles integration follow migration;
[scheduled reports](docs/reports.md) already support Simple Agents runs, composed
HTML email, manual previews and retained execution/delivery history.

Network deployment packaging and access controls: [deployment guide](deploy/README.md).

Telegram bot setup, report delivery and read-only investigations are managed in web Settings. See [Telegram integration](docs/telegram.md).
