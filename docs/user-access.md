# Users, dashboard sharing and browser authority

[TALIA-60](https://crumbles.lelloman.com/w/LLPR/TALIA/60) implements the agreed
admin/viewer model. This supersedes earlier proposals for independent per-user
source permissions and ordinary users authoring dashboards.

## Roles and ownership

LelloAuth authenticates people and controls admission to Talìa. Talìa stores its
own application roles keyed by the complete `issuer#subject`, never by username,
email or the first person to sign in. New accounts are viewers. An operator can
bootstrap named admin subjects with `TALIA_BOOTSTRAP_ADMINS` (comma-separated).
This inserts missing accounts only: restarting must not undo a later demotion.
Admins can promote/demote other known accounts in User access. Self-demotion is
rejected so the sole administrator cannot accidentally lock themselves out.
Roles, preferences and shares persist in the engine database (schema 11).

Admins author through separately authorized MCP agents and can administer all
engine data and dashboards. Existing machine credentials retain their independent
policies; possession of a browser session never grants MCP access. A global
operator is trusted to author on behalf of administrators.

Dashboard sharing metadata names an admin owner. Newly authored dashboards
without metadata are private admin drafts until an admin claims/shares them.
Existing dashboards are not automatically made public during migration.
Admins can make a dashboard public to all authenticated Talìa users, or share it
with specific OIDC subjects. Public never means anonymous internet access.
Deleting a saved dashboard deletes its share and clears account defaults pointing
to it; recreating its ID must not resurrect old access. Share changes require the
expected revision and an idempotent request ID, and are audited with the actor.
All admins are trusted administrators, rather than mutually isolated tenants.

## Dashboard as the data boundary

Opening a permitted dashboard grants a viewer access to **all raw read resources
listed in its saved package**, not merely the subset of a value displayed by a
widget. If only an aggregate should be exposed, the admin must publish that
aggregate as a separate engine resource and grant access only to it. No automatic
source-to-variable permission propagation is implied.

The server resolves the saved dashboard and checks current sharing, package
revision and its `grants.reads`; caller-supplied grant lists have no authority.
Viewer snapshots and subscription polls are filtered. Reads omit private variable
state, source/definition IDs and parameters. History is permitted only for approved
read resources. Whole-engine configuration, arbitrary resources, actions, setters,
writes, pipeline runs, authoring and live script control are rejected for viewers.
Subscriptions are isolated by authenticated subject and dashboard context;
revoked or no-longer-approved leases are released when encountered, or expire.
Access is rechecked after awaiting a getter before returning its result.

Client delivery checks access before returning a package, confirming it, or
selecting it. Viewer delivery removes write/run capabilities. Navigation and local
presentation interactions remain available. Admins should design shared dashboards
for read-only operation: the server cannot infer whether an arbitrary JS button
handler will attempt a write; such attempts fail even if a modified client tries
them directly. Server-side computed producers remain admin-authored server work.

`alerts` is an explicit read resource covering the alert snapshot/history exposed
by that API. Viewers without that grant cannot access the alert panel. Viewers
cannot acknowledge, silence, configure alerts or enroll notification devices.

Role and sharing decisions are read on every request. An already admitted admin
operation retains the existing operation-completion semantics; prior effects are
not undone. LelloAuth session/app revocation retains its existing 30-second
introspection window. A changed package revision requires viewer reload before
new data is delivered; old capabilities cannot authorize reads of a new package.
Already delivered data cannot be recalled from a person's browser. OIDC clients
must obtain authorized delivery to reload; they do not use offline package fallback.
A running dashboard may retain its last displayed samples during a network outage.

## Account default and client selection

Accounts have an optional saved default selected from permitted dashboards. A new
browser installation copies that default, or uses the first permitted dashboard
when no valid account default exists. No permitted dashboards produces an empty
state. Existing client/tab assignments remain unchanged when the account default
changes. Selection through the web shell updates this tab and the client default
for future tabs; already open tabs keep their own assignment. MCP can still set
explicit client/slot assignments, but it cannot bypass viewer delivery checks.
Clearing browser storage creates a new client, which uses the account default.
An ownership-checked `selectionState` request returns only the slot revision so
a viewer can replace a revoked assignment without obtaining its package. Slot and
client-default selection updates commit atomically. Viewer selection cannot inject
package parameter overrides. Display scaling remains
local to the client. The web shell exposes dashboard selection, account default,
reload/restart, connection/update state, identity/logout and client settings;
sharing and role administration appear only for admins. UI/VM authoring remains MCP.

## API and MCP

`POST /account` requires OIDC and the exact configured Origin. Operations:
`catalog`, `default {dashboard}`, admin-only `users`, `role {subject,admin}`, and
`share {access}`. Catalogs reveal only permitted dashboards; sharing metadata and
the user directory are admin-only. `POST /engine` includes dashboard context
`{id,revision}` for viewer requests. This is a selector, not a bearer capability.
The same context is required for viewer browser `POST /alerts` requests.

MCP `dashboard_access_list` returns policies and known user roles;
`dashboard_access_set` accepts `dashboardId`, `owner`, `public`, `viewers`,
`expectedRevision` and `requestId`. Both require global authoring save authority,
since publishing can expose data to every user. Narrow definition-authoring grants
cannot change sharing. Access mutation receipts are separate from engine-operation
audits and can be recovered by repeating the identical request ID.

The currently deployed OIDC transport is web. Native Android still uses the
existing development host transport; this change neither enables anonymous native
network access nor claims native OIDC enrollment is implemented.

## Verification

Run `cargo test --manifest-path engine/Cargo.toml --offline` with local mock sockets
allowed, then `bash dashboard/build-web.sh` and `node dashboard/tests/access.mjs`.
The latter launches a test-only full Rust service, local OIDC issuer and HTTPS
browser proxy, using ephemeral keys and data. The production binary does not
accept the fixture's HTTP OIDC issuer. It verifies empty viewers, role restrictions,
admin publishing, default selection on a new installation, server-side write/config
denials, responsive shell and active-access revocation. Existing web, responsive,
updates and delivery browser tests cover compatibility with development clients.
