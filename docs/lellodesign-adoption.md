# LelloDesign adoption

The web host uses the actual `@lelloman/lellodesign-vue` components, built from
LelloDesign commit `76d8f662e4df130f4c127615dd6f4450966e1536`. The published 0.1.0
package predates its header controls, so a commit-pinned package archive is
included with [provenance and upgrade instructions](../dashboard/vendor/README.md).
Vue 3.5.13 and the archive are locked by `dashboard/web/package-lock.json`.
Neither the sibling checkout nor registry credentials are needed to build Talìa.

The product uses Blue light/dark semantic palettes and these library components:

| Responsibility | Component |
| --- | --- |
| Theme and scoped native control styling | LelloTheme |
| Desktop sidebar/rail, mobile drawer, segmented header seam | LelloScaffold |
| Account identity at the navigation foot | LelloAccount |
| Account details and sign-out dialog | LelloDialog, LelloButton |
| Connection dot and accessible state label | LelloConnectionStatus |
| Light, Dark and System appearance | LelloThemeSelector |

Application navigation exposes Dashboard, Settings, permitted Alerts, and
admin-only Sharing and Users. This navigation is separate from the screens and
navigation authored inside a saved Dashboard. The sidebar is 224px expanded or
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

The declarative dashboard renderer still creates platform-independent widgets;
its native HTML controls inherit the scoped library styles and charts use the
primary color token. This is not a change to the Dashboard language or VM API.

## Verification and remaining clients

`dashboard/tests/access.mjs` exercises the actual Rust/OIDC boundary and checks
shared shell rendering, runtime identity retention, sharing draft preservation,
keyboard appearance selection, theme persistence/System changes, reduced motion,
mobile drawer account placement, resizing and narrow viewport overflow. Existing
VM, renderer, responsive layout, delivery and update tests remain regression checks.

This adoption covers the authenticated web shell. The server-rendered sign-in
landing page remains a small independent page; LelloAuth owns the authentication
screens. Native Android has not yet adopted the Compose library. Its next design
pass should consume LelloDesign Compose and the same Blue color family, account
placement and responsive scaffold rules while retaining native Views and the
shared dashboard language. Do not embed the web shell in a WebView.
