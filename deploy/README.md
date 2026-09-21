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
hashes are stored. Provider access tokens are encrypted at rest with a key derived
from the confidential-client secret. Browser sessions survive service restart;
changing issuer/client/secret/origin invalidates them. Session lifetime is bounded
by ID-token expiry and eight hours. Sign out invalidates the Talìa session;
LelloAuth's SSO session and other applications are unaffected.

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
MCP uses the existing operator credential, provisioned using `talia-agent` and
`operator-policy.json`; it does not use browser cookies or the OIDC client secret.

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
