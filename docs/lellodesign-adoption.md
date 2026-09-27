# LelloDesign adoption

The web host uses the actual `@lelloman/lellodesign-vue` components, built from
LelloDesign commit
`e0016d27ce4ccd69bfea2cbec941597859505bf1`, updated on 2026-09-26.
This is the unreleased open-workspace snapshot; its manifest still says 0.3.1.
The archive includes the approved Material Design 3 segmented buttons. See [provenance and upgrade instructions](../dashboard/vendor/README.md).
Vue 3.5.13 and the archive are locked by `dashboard/web/package-lock.json`.
Neither the sibling checkout nor registry credentials are needed to build Talìa.

The product uses Blue light/dark semantic palettes and these library components:

| Responsibility | Component |
| --- | --- |
| Theme and scoped native control styling | LelloTheme |
| Desktop sidebar/rail, mobile drawer, neutral header divider | LelloScaffold |
| Account identity at the navigation foot | LelloAccount |
| Account details and sign-out dialog | LelloDialog, LelloButton |
| Connection dot and accessible state label | LelloConnectionStatus |
| Light, Dark and System appearance | LelloThemeSelector |
| Single-choice CPU history range | LelloSegmentedControl |
| Open settings, sharing, users and alerts sections | LelloSection |

Application navigation exposes Dashboard, Settings, permitted Alerts, and
admin-only Sharing and Users. This navigation is separate from the screens and
navigation authored inside a saved Dashboard. The sidebar is 208px expanded or
80px collapsed; below 760 CSS pixels it becomes a drawer. These dimensions and
motion behavior come from the library. The real Talìa logo remains distinct from
the shared account avatar. Account identity is never placed in the app header.

Appearance and desktop collapse preference persist locally. System appearance
responds to OS changes. Host navigation, resizing, sidebar collapse and theme
changes retain the dashboard DOM island, its Worker, ViewModel and subscriptions.
Dashboard Reload/Restart keep their existing explicit semantics. Sharing drafts
are retained per dashboard during navigation and background catalog polling;
the original access revision remains attached to a draft, so external changes
produce a save conflict rather than silently overwriting them. Discard changes
restores the current saved policy. Server permissions remain authoritative.

The source of connection state is the engine transport; before a dashboard is
loaded it is account catalog connectivity. A static semantic dot is always
available in the header, with its label on hover, keyboard focus or tap. Engine
reconnection announcements remain available to assistive technology.

Dashboard, Alerts and Users use fluid operational workspaces with no global
editorial width cap. The library supplies 32 px desktop and 16 px mobile gutters
and 24/20 px desktop/mobile header titles and 20 px section titles. Settings and Sharing use locally constrained
640 px task groups with fields up to 480 px; explanatory prose uses a 72ch limit.
The host uses the library's `lv-workspace` helper. This follows upstream's product
layout families instead of applying the editorial 1120 px width to every page.

The declarative `SegmentedControl` node groups selected Button choices under an
accessible label. The web renderer mounts the actual LelloDesign Vue component
and maps selection to the existing VM actions. The library owns markup, keyboard
behavior and styling; Talìa owns state, chart data and placement. Removed controls
are unmounted, and distinct radio names isolate multiple dashboard controls.
Other native HTML controls inherit scoped library styles; charts use theme tokens.

## Verification and remaining clients

`dashboard/tests/access.mjs` exercises the actual Rust/OIDC boundary and checks
shared shell rendering, runtime identity retention, sharing draft preservation,
keyboard appearance selection, theme persistence/System changes, reduced motion,
mobile drawer account placement, resizing and narrow viewport overflow. Wide-screen checks cover 1920 px in light/dark,
expanded/collapsed navigation, aligned 64 px headers, fluid workspace width and
24 px titles. Settings fields remain locally bounded at 480 px and unsaved client
settings survive viewport changes. Existing
VM, renderer, responsive layout, delivery and update tests remain regression checks.

This adoption covers the authenticated web shell. The server-rendered sign-in
landing page remains a small independent page; LelloAuth owns the authentication
screens. Native Android has not yet adopted the Compose library. Its next design
pass should consume LelloDesign Compose and the same Blue color family, account
placement and responsive scaffold rules while retaining native Views and the
shared dashboard language. Do not embed the web shell in a WebView.

## Open-workspace upgrade

Inter is bundled locally with its OFL license. Settings, Sharing, Users and Alerts
use actual `LelloSection` components around retained form nodes; route titles
appear in the scaffold header instead of being repeated in content. The selected
dashboard name remains a useful section heading. Navigation focus moves to the
retained page region, and alert polling follows page visibility. The library owns
the neutral surfaces, navigation selection, control corners and section rules.
Browser checks verify font loading through the production asset packager, 208 px
sidebar, open sections, responsive light/dark layout and retained runtime state.

The consumer browser suite also exposed a late-edit race during dashboard library
fetching. The host now rechecks the dirty-edit guard immediately before replacing
the runtime, preserving edits made while a switch is loading.

## Deployment — 2026-09-26

Image: `registry.homelab:5000/talia:lellodesign-e0016d2-20260926`, digest
`sha256:c201b83dad192fe7c5f0b2e9509a26de84384e87e5884f378fa03eb646b8b7e9`.
Built from the working tree, including the previous report and Telegram changes.
Compose override: `/tmp/talia-lellodesign-e0016d2-20260926.yml` on homelab.
Future base-Compose `latest` deployments must include these changes.
Both pre-upgrade SQLite backups passed integrity checks. The complete
`dashboard/tests/access.mjs` real-browser suite passed, including the production
font response/MIME type, unique section labels and late-edit preservation checks.
