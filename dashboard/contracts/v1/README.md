# Dashboard contract v1

Implementation contract for P1. Both clients consume the same compiled UI and
JavaScript source; native appearance may differ. The P0 [runtime contract](../../../docs/runtime-contract.md)
still governs execution and authority. This is not a production deployment API.

## Package and revisions

A package has `version: 1`, `id`, `revision` (nonempty strings), `ui` (compiled
node), `viewModel` (UTF-8 JavaScript), and `definitions` (UI templates). Clients
choose their package and composition independently. A saved revision is immutable.
A newer revision, including changes in referenced definitions, displays **Update
available**. Explicit Reload takes one coherent latest package, resets local state
and discards temporary edits. No automatic adoption or engine rollback. Restart
uses that same saved-baseline path. Process recreation loads saved definitions.

References contain a definition name and JSON parameters. Updating the definition
updates all references on their next explicit reload; changing an instance's
parameters affects only that instance. Definition cycles and unknown references
are errors. UI instances have scoped IDs. ViewModel instances and function
references use independent state and explicit parameters, never copied definitions.

## Restricted source

Source is a single `<Dashboard id="...">` element. XML-like opening/closing tags,
self-closing tags, double-quoted strings (JSON escapes), and `{expression}` property
values are allowed. No text nodes, comments, imports, spreads, function calls,
arithmetic, assignments or arbitrary executable expressions. Whitespace is ignored
between tags. Expressions are JSON scalar literals or dotted paths rooted at
`state`, `params`, `item`, or `actions`. Unary `!` is allowed only for boolean
bindings. Properties starting `on` must name an `actions.name` handler.

Every node has an explicit ID, unique in its definition. A compiled node is
`{type,id,props,children,source:{line,column}}`. Literal properties are JSON
scalars; bindings are `{bind:"state.path",not:false}` and events are
`{action:"name"}`. The compiler produces `{version:1,root:node}`. Consumers reject
unsupported versions, unknown components/properties and invalid property types.
Source diagnostics identify line and column. Source is parsed, never evaluated.

## Vocabulary

| Element | Properties beyond id | Meaning |
|---|---|---|
| Dashboard | — | Contains Screen definitions and exactly one Surface |
| Screen | — | Named screen, one layout child |
| Surface | — | One layout child composing ScreenRefs |
| ScreenRef | screen | String screen ID, literal or binding |
| Column, Row | gap, padding, width, height, visibility | Linear ViewGroups |
| Grid | columns, gap, padding, width, height, visibility | Equal-width columns |
| Scroll | width, height, visibility | One child, vertical scrolling |
| Text, Status | text, label, visibility | Wrapping text; Status is a live status |
| Chart | values, label, height, visibility | Finite numeric array, line chart and accessible summary |
| Button | text, enabled, onClick, visibility | Named action |
| Slider | value, min, max, step, label, enabled, onChange, visibility | Numeric event value, accessible label required |
| Switch | value, label, enabled, onChange, visibility | Boolean event value, accessible label required |
| If | when | Boolean condition; one child, collapsed when false |
| For | items, key | Array binding, key is an item path; one child template |
| Use | definition, params | UI definition reference and JSON-object binding |
| Width | min, max | Conditional responsive branch; one child |

Defaults: visibility `visible`, enabled true, gap/padding `0dp`, slider step 1,
grid columns 1. Required fields are evident from semantics: text, chart values and
label, control value/range/label/handler, screen target, condition, repeated items
and key, reference name/parameters. Empty For is valid. Missing binding paths,
wrong runtime types, missing/duplicate repeated keys are visible internal errors.
Keys must be nonempty strings or finite numbers and unique within the For.
Identity is the ancestor instance path + definition node ID + typed repeat key.
Reorder preserves the native/DOM control and focus. Different reference instances
cannot collide. Hidden keeps space; collapsed removes space. Neither pauses VM.

Lengths are nonnegative strings with explicit `dp` or `px`. Layout width/height
may also be `fill` or `auto`. Web px means CSS px. Web dp is multiplied by a
positive, manually configured per-client scale (default 1); physical display size
is never inferred. Android uses native density for dp and physical layout pixels
for px. Width branches test available client surface width after conversion:
`min` inclusive, `max` exclusive. Both optional, at least one required. Branches
are independent; authors can deliberately show multiple matches. Changing width
or scale does not recreate VM or reset selected screen. Navigation chrome is a
Screen composed alongside destination ScreenRefs on one Surface. Dashboard VM
and subscriptions persist across screen navigation, even when a screen is absent.

## ViewModel and events

Source calls `defineVM({initial(params), actions, start?})`. `initial` returns
plain JSON state. Each action is `(ctx,event) => value | Promise`; optional start
receives ctx. `ctx.state()` returns `{revision,value}`; `ctx.commit(snapshot,next)`
performs a short synchronous compare-and-set. Stale snapshots fail immediately,
without automatic retry. I/O yields; other actions run while awaiting. An action
may obtain a new snapshot after awaiting when it deliberately wants current state.
`ctx.read(key)`, `ctx.write(value)`, `ctx.subscribe(key, handlerName)` use granted
engine capabilities. Subscription events use the named action with `event.value`.
An action event is `{target:string,value:JSON}`; click value is null. Hosts validate
the current control's event, range and enabled state before dispatch. Scripts do
not receive UI objects. Live script edits only affect VM and mark the instance dirty.

`ctx.instance(name, definition, params)` resolves a shared VM definition with
independent state. Its `dispatch(action,event)` and `state()` access that instance.
`ctx.fn(name,...args)` resolves a shared function definition with parameters passed
explicitly. No network imports. Definitions are packaged with the saved revision.

## Host envelope and lifecycle

Guest/host messages use `{id:positiveInteger,op,value}` and replies use
`{id,value}` or `{id,error}`. Supported engine operations are read, write,
subscribe, unsubscribe. A subscription event is `{event:subscriptionId,value}`.
Host-owned identity, grants, epochs and action IDs are never accepted from guest
labels. Read/write return promises. Subscribe delivers current snapshots and later
revisions; coalescing is allowed, events are not durable historical delivery.
External operation failures reject promises and can be handled by VM actions.
Uncaught errors stop the dashboard, show diagnostics and offer Restart. Optional
failure signals are host configuration. Running effects are never rolled back.

Backgrounding the client pauses guest execution and subscriptions. Resume reads a
fresh snapshot and reconciles issued action IDs; it never resubmits writes. Old
responses are fenced by epoch. Retained guests preserve local state/dirty edits;
process death does not. Host enforces source/message/memory/CPU/pending limits.
Cancelled invocations cannot commit, publish or dispatch further effects. Local
atomicity applies within this frontend instance; server state has server authority.

See [the example](../../examples/monitor.ui) and its
[ViewModel](../../examples/monitor.vm.js). These are shared authoring inputs, not
HTML or Android layouts. Compilation and host qualification are separate steps.
