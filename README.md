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
are persisted; SQLite remains a storage candidate. Clients access this server
engine through an API rather than hosting their own monitoring engine.
Variables carry typed values and quality metadata with configurable history.
Computed values support stateful getters/setters, dependency updates and caching;
async operations may interleave on the same instance, with short atomic state
updates and configurable shared-refresh or independent-read behavior.

These documents record the **high-level product direction discussed with the
user**, not a signed-off detailed specification or implementation architecture.
An isolated [runtime prototype](docs/runtime-prototype.md) has been implemented
and tested; the product has not been implemented or deployed.

- [Runtime findings](docs/runtime-prototype.md): executable P0 evidence and
  unresolved qualification gaps.
- [Implementation plan](docs/implementation-plan.md): first milestone with both
  web and native Android, technical prototypes, and acceptance gates.
- [Product specification](docs/specification.md): discussed scope and open
  behavioral questions.
- [Server engine](docs/engine.md): runtime primitives, execution, persistence,
  and reusable definitions with independent instance state.
- [Dashboard model](docs/dashboards.md): shared UI and logic, client composition,
  engine bindings, and persistent versus live MCP operations.
- [Architecture boundaries](docs/architecture.md): integration responsibilities
  and decisions still to make.
- [Existing systems and migration considerations](docs/migration.md): source
  inventory and issues to address when planning the transition.

- [Brand identity](docs/branding.md): selected Wide gaze icon, assets and exploration.
