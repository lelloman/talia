# First network deployment

Build from a clean source commit using `deploy/Dockerfile`. Homelab owns Compose,
Caddy, private DNS, image tagging and deployment. The image contains the Rust
engine and an explicit selection of web assets; it does not run `dashboard/serve.py`.

Required configuration:

- `TALIA_ACCESS_TOKEN_FILE`: mounted private file containing 32 cryptographically
  random bytes encoded as 64 hexadecimal characters. Generate once and retain
  across updates. Never include it in an image, URL or deployment record.
- `TALIA_PUBLIC_ORIGIN`: exact HTTPS origin, without a trailing slash.
- `TALIA_WEB_ROOT`: packaged assets (`/opt/talia/web` in the image).
- `TALIA_LISTEN`: container bind address (`0.0.0.0` in the image).
- Arguments: persistent SQLite path and port, normally `/data/talia.sqlite3 8080`.

Missing network authentication configuration fails startup. Without deployment
configuration, existing development tools remain loopback-only. The proxy must
terminate TLS; the container listener must not be published to an untrusted network.

The first release has an operator access key, not multi-user browser sign-in.
It grants trusted dashboard access to the engine, including writes and runs;
it does not provide per-user engine resource authorization. Browser sign-in sets
an eight-hour Secure, HttpOnly, SameSite=Strict cookie. Host integrations may use
`X-Talia-Access`; never expose the key to guest JavaScript. Rotate the file and
restart to invalidate all sessions. Cross-origin browser writes are rejected.

Agent and alert APIs retain their own independent principal permissions; the
operator access key cannot replace their Bearer credentials. Use the offline
`talia-agent` binary with `operator-policy.json` to provision an initial operator
credential while the engine is stopped. The output must be a new private file.
Prefer narrower principals for integrations. Client registration retains its
installation identity but now also requires deployment access.

`/healthz` is public and checks the running request queue and database snapshot,
returning no application data. The deployment root only contains selected static
assets. Data, configuration, source files and build outputs are not web content.

The optional `--seed` argument installs the existing example dashboard for smoke
checks. It is sample data, not homelab monitoring. Do not interpret it as migrated
coverage. Run `talia-bootstrap DATABASE` once while stopped to create a welcome dashboard
with no fabricated metrics. It refuses to overwrite an initialized catalog. Android's current host configuration
still uses its development loopback endpoint; remote Android onboarding and live
push-provider setup are separate from bringing up this server/web service.

SQLite state includes definitions, observations, registrations, policies and
alert delivery state. Preserve the database and its WAL together. Stop the service
before a cold copy, or use SQLite's backup API for a consistent online snapshot.
Verify `PRAGMA integrity_check` on a restored copy. Never overwrite a live database
or rewind accepted side effects. Before upgrades retain a consistent backup and
the current image digest; old binaries may not support a newer schema. First
installation rollback is stopping the new service and removing its route while
retaining its data. Subsequent image rollback requires schema compatibility.

Verification: build the web assets and Rust binaries, then run
`python3 deploy/test-service.py`. It exercises the access boundary, origin checks,
static isolation, registration, health, restart and a real Chromium dashboard.

First installation: [deployment record](first-deployment.md).
