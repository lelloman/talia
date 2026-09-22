# Roadmap and planning ownership

**Crumbles project `LLPR/TALIA` is the authoritative planning backlog.** Scope,
acceptance criteria, open decisions, sequencing, progress and implementation
subtasks belong in its Stories. This file is a navigation index, not a second plan.

| Stage | Story |
|---|---|
| P0 — Runtime and transport foundations | [LLPR/TALIA-1](https://crumbles.lelloman.com/w/LLPR/TALIA/1) |
| P1 — Shared UI and ViewModels on web and native Android | [LLPR/TALIA-2](https://crumbles.lelloman.com/w/LLPR/TALIA/2) |
| P2 — Durable, runtime-configurable server engine | [LLPR/TALIA-3](https://crumbles.lelloman.com/w/LLPR/TALIA/3) |
| P3 — Collection Pipelines and stateful Watches | [LLPR/TALIA-4](https://crumbles.lelloman.com/w/LLPR/TALIA/4) |
| P4 — MCP authoring and live dashboard control | [LLPR/TALIA-5](https://crumbles.lelloman.com/w/LLPR/TALIA/5) |
| Configurable alerts and notification delivery | [LLPR/TALIA-47](https://crumbles.lelloman.com/w/LLPR/TALIA/47) |
| P5 — Complete development-release demonstration | [LLPR/TALIA-6](https://crumbles.lelloman.com/w/LLPR/TALIA/6) |
| Production qualification | [LLPR/TALIA-8](https://crumbles.lelloman.com/w/LLPR/TALIA/8) |
| Homelab migration and cutover | [LLPR/TALIA-9](https://crumbles.lelloman.com/w/LLPR/TALIA/9) |
| Simple Agents and Crumbles integration (after migration) | [LLPR/TALIA-7](https://crumbles.lelloman.com/w/LLPR/TALIA/7) |

## Working agreement

Create actionable Sub-task children **before starting implementation of a Story**,
not a speculative full subtask backlog in advance. Use Crumbles
`prepare_ticket_refinement` to review the story, current repository evidence and
unresolved decisions. Refine child scope, acceptance criteria, verification,
repository ownership and execution order before implementation. Assignment and
normal implementation/review workflow apply to that resulting work.

The Stories record existing work separately from remaining work. P0 includes the
completed runtime/transport experiments and unresolved qualification gaps; it is
not marked complete merely because those experiments passed. Alert qualification
follows P4; release preparation and production readiness lead into homelab migration.
Consult the tickets for current status rather than updating another checklist here.

## Repository documents

The [specification](specification.md), [architecture](architecture.md),
[engine model](engine.md) and [dashboard model](dashboards.md) retain product and
technical reference material. Open-question sections are inputs to Story refinement,
not independently maintained backlogs. Update these references when decisions are
resolved in Crumbles; ticket creation does not sign off provisional API choices.

[Runtime](runtime-prototype.md), [execution](execution-policy-prototype.md) and
[transport](transport-prototype.md) findings retain implementation evidence.
The [migration inventory](migration.md) retains historical source observations;
current rollout planning belongs in LLPR/TALIA-9.

The previous detailed implementation plan remains in Git at commit `c3c3e22`.
Its stage scopes, acceptance scenario and unresolved planning topics were migrated
to the Stories above on 2026-09-19. No production deployment, migration or legacy
service retirement is authorized by this planning migration.

Scheduled reporting workflows, including direct Simple Agents execution for report
composition, are implemented in [TALIA-68](https://crumbles.lelloman.com/w/LLPR/TALIA/68).
This advances the reporting subset of agent integration; general investigations,
outcome triggers and Crumbles delegation remain in the later integration workstream.
See [report contract and setup](reports.md).
