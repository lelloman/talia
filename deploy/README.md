# Network deployment with LelloAuth OIDC

Talìa uses a confidential LelloAuth client with authorization-code flow and S256
PKCE. Sign in with a LelloAuth account granted access to the `talia` app.
There is no shared deployment key, token-entry form or key-header fallback.

## Configuration

Homelab owns Compose, Caddy, private DNS and registry builds. Required settings:

- `TALIA_OIDC_ISSUER=https://auth.lelloman.com`
- `TALIA_OIDC_CLIENT_ID=talia`
- `TALIA_OIDC_SECRET_FILE=/run/talia/oidc-client.secret`
- `TALIA_AUTH_DB=/data/auth.sqlite3`
- `TALIA_PUBLIC_ORIGIN=https://talia.lan.lelloman.com`
- `TALIA_WEB_ROOT=/opt/talia/web` and `TALIA_LISTEN=0.0.0.0`

Register the exact redirect URI
`https://talia.lan.lelloman.com/auth/callback` in LelloAuth. Keep the client secret
private, mode 0600, owned by container UID 65532. No provider credential enters
JavaScript or the dashboard VM. Missing OIDC configuration fails startup for
network deployments; the removed `TALIA_ACCESS_TOKEN_FILE` is explicitly rejected.
Loopback development without deployment configuration remains available.

Discovery and signing keys are validated with bounded HTTP requests, no redirects,
RS256 signature/issuer/audience/time checks, nonce and browser-bound one-use state.
The implementation adapts the existing Simple Agents OIDC validator and its tests.
Protocol reference: [OpenID Connect Core](https://openid.net/specs/openid-connect-core-1_0.html).

## Sessions and permissions

Opaque individual sessions use Secure/HttpOnly/SameSite cookies. Only session
hashes are stored. Provider access and refresh tokens are encrypted together at
rest with a key derived from the confidential-client secret. Browser sessions
survive service restart; changing issuer/client/secret/origin invalidates them.
New renewable sessions last 30 days, with automatic server-side access-token
refresh on demand within 30 seconds of expiry. The deadline is absolute, not
extended by activity. Sign out invalidates the Talìa session;
LelloAuth's SSO session and other applications are unaffected.

Rotation is serialized per browser session across tabs, browser APIs and agent-key
checks; unrelated sessions can refresh concurrently. The encrypted successor is
saved before introspection. In-flight logout never recreates a deleted session.
A durable pending marker prevents reuse of a refresh token after an interrupted
rotation (crash, cancellation, lost response); that session must sign in again.
Explicit provider 5xx responses can be retried, while failed identity validation,
invalid grants and ambiguous transport failures require a new login. A temporary
introspection outage retains the session and its latest token pair while denying
requests until validation succeeds.

Existing sessions from before this change contain only an access token. They
retain their original short deadline: sign in once after upgrading to obtain a
renewable session. A provider that omits refresh tokens retains the old bounded
session behavior. No auth table/schema change is needed; the encrypted payload
format changed. Back up both databases before upgrading. Rolling back the binary
cannot use new payloads and requires affected users to sign in again; preserve
the current engine/auth databases rather than restoring stale application data.

LelloAuth introspection verifies current user/app/token access on sign-in and at
most every 30 seconds during use. Revocation fails closed; provider outages return
service unavailable when revalidation is due. Unsafe browser requests require
the exact configured Origin, including logout. Cross-user browser installation
credentials are scoped by OIDC subject, with separate client registrations.

New Talìa accounts are read-only viewers. Configure `TALIA_BOOTSTRAP_ADMINS` with
explicit `issuer#subject` identities for initial administrators; this only inserts
missing users and never reverses later role changes. Admins share dashboards through
the web shell or MCP. Viewers can read only the resources of permitted dashboards;
whole-engine access and all server-data mutations are denied. Browser alerts require
the dashboard's `alerts` read grant, and only admins can acknowledge or silence.
See [user access](../docs/user-access.md) for ownership, sharing and account defaults.
Alert configuration and agent operations keep their independent machine grants.
Interactive agents use [temporary account-bound keys](../docs/agent-access.md)
created in Settings and connect to the HTTPS `/mcp` endpoint. The legacy stdio
adapter uses its existing operator credential, provisioned using `talia-agent`
and `operator-policy.json`. Neither transport exposes the OIDC client secret.

## Installation and recovery

The packaged Rust service serves only selected web assets. `/healthz` checks its
request queue and engine database without exposing data. Run `talia-bootstrap
DATABASE` once while stopped to create a welcome dashboard; it refuses to
replace an initialized catalog. Do not seed fabricated metrics in deployment.

Preserve both `talia.sqlite3` and `auth.sqlite3`. Use SQLite's backup API or stop
the writer for consistent copies; verify restored copies with `integrity_check`.
Keep the confidential-client secret and machine credentials in a separate private
recovery set. Removing auth.sqlite3 signs everyone out; it does not delete engine
state. Never roll back to the rejected deployment-key image.

The original [first-install record](first-deployment.md) is historical. OIDC
supersedes its login instructions. Native Android remote onboarding and live
notification-provider configuration are separate work.

Run `python3 deploy/test-service.py` for protocol/session regression tests. Live
Historical OIDC-only browser qualification uses `deploy/verify-oidc.mjs` with a temporary account
that has access only to Talìa; remove that account after verification.

Dashboard access qualification: `deploy/verify-access.mjs` uses a temporary viewer,
MCP sharing, denied direct-engine operations, a second browser installation and
share revocation. Its private fixture credentials are outside Git; delete the
temporary identity after the run. Local reproducible coverage is in
`dashboard/tests/access.mjs` and does not require real provider accounts.

## Optional browser notifications

Set `TALIA_ALERT_PROVIDERS` to a private JSON file mounted in the container and
mount a persistent P-256 VAPID private key readable by UID 65532. The complete
provider format and enrollment lifecycle are in
[Browser Web Push destinations](../docs/alerts.md#browser-web-push-destinations).
Adding environment variables or mounts requires recreating the container;
subsequent provider-file changes are picked up without restarting Talìa.
After deployment, an administrator enables browser notifications in Settings and
an alert policy targets the displayed destination ID. Real vendor delivery and
OS permission must be qualified on that browser; local fixtures do not prove it.

## Scheduled reports and Simple Agents

Reports use the existing SMTP destinations/provider file plus an optional private
`TALIA_REPORT_AGENTS=/run/talia/report-agents.json` map and Simple Agents token file.
Provision the caller/profile on Simple Agents, mount those files read-only for UID
65532, and recreate the container to add the environment variable. Configure and
preview workflows through MCP. See [scheduled reports](../docs/reports.md) for the
provider format, complete example and restart/delivery semantics. Migration 13
adds report tables; retain a database backup before upgrading.
