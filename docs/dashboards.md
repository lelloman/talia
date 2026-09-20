# Dashboard model

Status: decisions recorded from discussion, 2026-09-18. The boundaries and
language direction below are agreed; the grammar, runtime and detailed behavior
still need specification. Examples illustrate intent, not an implemented schema.

Planning, open-decision tracking and execution status live in the
[Crumbles Stories](implementation-plan.md). This document is technical reference;
its open questions are inputs to Story refinement.

## One definition for UI and logic

A dashboard is a configurable application interface. It includes both declarative
UI and scripted behavior, usable on web and native Android without separate
platform-specific dashboard source.

Each client instance selects its own dashboard configuration. A phone can use
bottom navigation while a browser uses a sidebar, but both configurations use
the same language and engine contract. Definitions can be reused across clients;
assignment, sharing and synchronization details remain open.

Humans view and interact with dashboards. Agents author and modify their saved
definitions through MCP. This replaces the earlier assumption of a human-facing
layout editor. Buttons, sliders, switches and other controls remain fully
interactive for users; agent-driven authoring does not mean a read-only UI.

## Hierarchy and composition

| Element | Meaning |
|---|---|
| Dashboard | Complete configuration selected by a client, including UI, logic, screen composition and navigation |
| Screen | Named, composable interface area; not necessarily a full page or navigation destination |
| ViewGroup | Layout container holding Views and optionally nested ViewGroups |
| View | Individual visual or interactive element |

Several screens may appear on the same surface. For example:

```text
Dashboard
├── Navigation screen
│   └── ViewGroup
│       ├── View: Home link
│       └── View: Infrastructure link
└── Selected content screen
    └── ViewGroup
        ├── View: CPU chart
        ├── View: Time-range slider
        └── View: Investigate button
```

Sidebars with icons, header tabs, bottom navigation and links are examples of
configurable navigation. Placement and composition belong to the client's
selected dashboard definition, rather than being fixed application chrome.
Named screen slots are a candidate mechanism, not yet a selected syntax.

## MVVM responsibilities

| Layer | Responsibility |
|---|---|
| View | Declarative Screen/ViewGroup/View trees, layout, bindings and event mappings |
| ViewModel | JavaScript state, computed values, subscription handling and user-event logic |
| Model / engine | Stable monitoring data and capabilities exposed through read, write and subscribe |

The dashboard definition contains the View and ViewModel. The engine is
independent of that definition, allowing dashboards to change frequently and
radically without redefining the underlying monitoring operations.

Local UI behavior, such as a selected tab or an unsaved input, belongs to the
ViewModel. Persistent configuration changes and operational actions go through
the server engine through a client API/SDK. The engine is Rust/Axum with embedded
JavaScript for server Pipeline and Watch logic. Client ViewModel subscription
reactions are local and separate from persistent server Watches.

## Local computed state and server values

Local frontend getters, setters and reads may await. Other operations, including
those on the same local instance, can run during the wait. Only short synchronous
state updates are atomic; they cannot contain `await`. Separate frontends retain
independent local state. A remote server Variable has one authoritative state-update
boundary at the server; accessing it through a client API does not create a separate
writable copy or make a sequence of remote calls atomic.

Computed values configure shared refresh or independent reads. Sharing an in-flight
getter does not block setters or other operations. Resumed operations must handle
changed state before publishing results, and cancelled operations must not publish
late results. Stale evaluations fail explicitly without automatic retry. Every definition selects a read mode; cancelling the last shared reader cancels its producer. Exact API names and cache freshness remain open.

The [runtime contract](runtime-contract.md) defines background pausing, outcome reconciliation, visible internal failures and manual Restart. External operation failures remain recoverable data/errors.

The [computed-value model](engine.md#variables-and-computed-values) supports a
getter, optional setter, internal state and a time provider. Persistence of
server-side computed state does not imply persistence of temporary dashboard
ViewModel changes: the local dirty/reload boundary remains unchanged.
Multi-value transactions, rollback and atomic external effects are not implied
by atomic state updates.

## Reusable definitions

UI elements, ViewModel elements and functions support the same reference model
as [server Watches](engine.md#shared-definitions-and-instances). A shared definition
provides structure or behavior and configurable parameters. Instances reference
it with their own parameters and independent state where applicable.

Editing a shared definition updates all references; editing one instance's
parameters affects only that instance. This is reuse by reference, not copying.
Running dashboards show Update available and adopt a coherent saved revision only
on explicit reload, resetting local state and discarding temporary edits. These are persistent authoring changes, not permission for live MCP
to edit View definitions. Local temporary script changes are not shared edits.

## Authoring language

The chosen direction is **restricted JSX-like UI syntax with separate JavaScript
ViewModels**. Talìa parses and validates the UI into a platform-independent typed
tree, which each client renders using its platform's UI facilities.

This is not a requirement to use React, HTML elements, or the browser DOM as the
shared UI model. The UI language has a documented component vocabulary, typed
properties, simple binding expressions, explicit conditional/repetition
constructs, and named ViewModel handlers. General behavior lives in the
ViewModel, rather than arbitrary executable blocks embedded in layout markup.

Illustrative UI:

```jsx
<Column id="controls" gap={12}>
  <Text text="Investigation window" />
  <Slider
    id="period"
    min={1}
    max={24}
    value={state.periodHours}
    onChange={actions.setPeriod}
  />
  <Button
    id="investigate"
    text="Investigate"
    enabled={!state.investigating}
    onClick={actions.investigate}
  />
</Column>
```

This early example illustrates the direction. The implemented
[contract v1](../dashboard/contracts/v1/README.md) is authoritative for exact
syntax, vocabulary, explicit dp/px units, IDs and validation diagnostics.

Web and Android implement common layout, interaction and JavaScript semantics.
Responsive definitions use available surface width, with platform-native controls.
Contract v1 defines sizing, labels, conditions and stable keyed repetition.

## Engine contract

Both ViewModels and MCP access engine capabilities:

- **Read:** query metrics, retrieve results, inspect sessions or tickets.
- **Write:** change persistent configuration or perform an action, such as
  starting a check or issuing a ticket.
- **Subscribe:** receive data, task progress, alert or configuration updates.

These are categories of operation, not finalized method signatures. A chart
can bind to subscribed data; a button can invoke a write; a switch can read and
update a setting. The engine remains the authority for its state and operations.
The transport and permissions still need definition. Authority lives in the
server engine; the client API is a bridge, not a second monitoring engine.
Client reload does not reset server Watches, Variables or running monitoring.

## Two MCP capabilities

| Capability | Scope | Persistence |
|---|---|---|
| Engine access | Invoke engine reads, writes and subscriptions directly | Depends on the engine operation; writes can have persistent effects |
| Live dashboard interaction | Execute code or modify logic/state in the ViewModel of a specific running instance | Temporary; modifying the ViewModel makes the instance dirty |
| Dashboard authoring | Create or edit the saved UI and ViewModel definition | Persistent; defines what clients load |

The two dashboard capabilities—live interaction and authoring—are separate.
Direct engine access is also available without manipulating a dashboard.

Live interaction cannot modify the running dashboard's View definitions. Existing
bindings, conditional content and repeated items can change what is displayed as
state changes; this is not permission to rewrite the underlying View tree.

Live ViewModel modifications are not automatically saved or promoted into the
persistent definition. Only reload restores a clean instance from the saved
definition and discards those temporary modifications. This does not mean that
ordinary user interaction marks a dashboard dirty; dirty denotes live MCP/script
modification rather than normal execution of its authored behavior.

**Reload is not an engine rollback.** A ticket created, task started, or persistent
setting changed through the engine survives reloading the dashboard. Likewise,
external data may have changed, so reload restores authored behavior and initial
local state, not a historical snapshot of the world.

Live MCP operations need to target a particular running client/dashboard
instance. Saving a definition is distinct from applying it to a live instance;
explicit reload adopts the latest coherent saved revision.

## Historical open details and remaining scope

The list below predates P1. Contract v1 and the [P1 implementation](../dashboard/README.md)
now resolve the initial component grammar, bindings, lifecycle, responsive layout,
reference scoping and explicit revision adoption. Production publishing, client
assignment and authenticated MCP tools remain later work.


- Exact components, properties, units, layouts, responsive and accessibility rules.
- UI grammar, allowed expressions, conditional and repeated-content syntax.
- ViewModel state/reactivity API, lifecycle, scoping across screens and error handling.
- JavaScript runtime choice, supported language features, isolation, execution
  limits, and subscription/resource cleanup on reload.
- Engine operation signatures, types, errors, permissions and stream semantics.
- Live code execution semantics, what counts as a modification, inspection-only
  calls, concurrent manipulation and the user-visible dirty indicator.
- Dashboard definition versioning, publishing, client assignment, and which saved
  revision a reload uses when authoring has occurred concurrently.
- Exact MCP tools for discovery, validation, preview, editing and live targeting.
- Whether authors interact through an assistant inside Talìa, external MCP agents,
  or both; this has not been selected.

## P0 lifecycle implementation evidence

The [lifecycle fixture](../spikes/lifecycle/README.md) now exercises actual browser
visibility and Android Activity transitions against the loopback server. Pausing
stops local guest pumping and snapshot polling; resumed clients refresh and query
recorded action IDs rather than resubmitting writes. Host epochs reject old replies.
Retained runtimes keep temporary edits, while process recreation/reload restores
the saved source. This qualifies the recorded fixture paths on Chromium and the
Android emulator, not the future P1 renderer or physical-device lifecycle coverage.

The fixture also implements visible stopped/error state, opt-in failure signals
and manual saved-baseline Restart. Script exceptions, runaway execution and memory
exhaustion were exercised with an unrelated surviving guest on Chromium and native
Android x86_64. External probe errors remain recoverable. Recorded evidence is in
`spikes/lifecycle/results/failures`; full notification delivery is not implemented.

The subsequent [combined qualification run](p0-qualification.md) now passes on
Linux, Chromium, Android x86_64 and physical ARM64, including the newer capability,
execution, lifecycle and failure adapters. Earlier reports and their stated limits
remain historical; the combined report supplies current cross-platform evidence.

## P1 implementation contract

The [versioned UI/VM contract](../dashboard/contracts/v1/README.md) defines the
restricted grammar, initial vocabulary, bindings and host envelopes. Navigation
preserves dashboard state and subscriptions. Width rules are dashboard-authored
in dp or px; web px means CSS pixels and web dp uses a manual per-client scale.
Hidden retains space, collapsed removes it, and neither pauses the ViewModel.
Repeated content requires stable unique keys and preserves control identity.
This contract supersedes the earlier illustrative/open details for those topics.
