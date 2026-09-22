# Temporary agent access

Sign in to the deployed web app and open **Settings → Agent access**. Create an
agent key, then copy either the key or the JSON MCP configuration into your agent
harness. The endpoint is `https://talia.lan.lelloman.com/mcp`; it is reachable on
the home network or VPN. Select **Streamable HTTP** and supply
`Authorization: Bearer <key>`. No SSH or local Talìa executable is needed.

Keys last one hour, support multiple calls and reconnects, and cannot be extended.
The secret is displayed only at creation. It is not saved in browser storage or
passed to the dashboard VM. Leaving Settings, hiding the key, or expiration clears
the displayed secret. Clipboard contents are controlled by the user/harness.
Create another key when needed. The active-key list exposes creation/expiry times
and a Revoke button, never the secret. Up to ten unexpired keys per user and 1,024
in total are admitted.

Keys are bound to both the account and the OIDC login that issued them. Signing
out of that login or its expiration invalidates its keys; closing a tab does not.
Provider app/session access is revalidated under the same bounded 30-second cache
as browser access, failing closed when revalidation cannot succeed. Other logins
can list and revoke the account's keys, but cannot retrieve their secrets.

Current account permissions apply on every operation. Administrators can use the
existing authoring, engine, live-control and alert tools. Viewers remain limited
to dashboards as presented in the UI: their MCP tool list is empty and direct
calls cannot expose arbitrary sources, definitions or engine data. Promoting or
demoting an account changes key authority without issuing a new key. Audited agent
operations use the exact OIDC `issuer#subject` as principal. Key expiry/revocation
prevents further admission and delayed response delivery; already committed or
external effects are not undone.

## Transport and implementation

`/mcp` uses the pinned official Rust MCP SDK's Streamable HTTP service with
stateless JSON responses, host/origin validation and bounded request admission.
There is no SSE push stream or transport-session resume buffer; engine subscription
leases retain the existing explicit polling interface, scoped to the key. The
full random key ID supplies the internal 64-hex connection identity used by
engine and live tools; reconnecting with the same key preserves its leases, and
a different key cannot poll them even when issued to the same account. The
endpoint authenticates every request using the Bearer header; browser cookies,
URL query tokens and permanent operator credentials are not accepted as substitutes.
The standard initialize, notifications, tools/list and tools/call exchange is
verified using the independent official TypeScript MCP client SDK.

Browser key management uses same-origin OIDC-protected `/account` operations
`agentKeyCreate`, `agentKeys` and `agentKeyRevoke` (with `id`). Only the create
response contains `key`. Keys have 256 random bits; SQLite stores only the SHA-256
digest, account, opaque originating session reference, and expiry metadata.
Database schema 12 adds `user_agent_keys`; existing operator policies are unchanged.
The legacy stdio adapter and its `/agent` bridge remain available for operator
automation and do not accept these temporary keys.

## Verification and recovery

`node dashboard/tests/access.mjs` exercises the real Rust service with a local OIDC
provider and HTTPS proxy: creation/copy UI, SDK discovery and authoring, attribution,
engine reads, key-isolated subscription polling across reconnects, live inspection,
execution and reload (including dirty guards), viewer isolation, wrong-origin/protocol rejection, cross-user revoke isolation,
expiry, revocation and logout. `cargo test --manifest-path engine/Cargo.toml`
also checks role changes and credential separation. Fixtures never send real alerts.
The MCP client SDK is a web dev dependency only; deployment builds omit it.

Back up both engine and auth databases before upgrading. An older schema-11 engine
will refuse schema 12; rollback therefore requires the matching pre-upgrade engine
and auth database backups, not only changing the container image. Keep machine
credentials and the OIDC client secret in their existing private recovery set.
