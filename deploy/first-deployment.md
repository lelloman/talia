# First homelab deployment — 2026-09-21

Talìa is running at **https://talia.lan.lelloman.com** on private LAN/VPN access.
The service follows homelab's Compose/private-registry/Caddy pattern. Publicly
trusted TLS uses the existing Cloudflare DNS-01 issuer. No public address or VPS
route was added. Home DNS resolves the name to 192.168.1.101.

## Deployed artifacts

- Application image source: `dee6dd2dc5ab229f9dbc2902ce38823acee3e168`.
- Registry digest: `sha256:6ae1f36e696a4e34b0b38bd1f6421b0f3d3cdf92dc717d02542e27d83d24ba7d`.
- Homelab integration commit: `1989f1d`.
- Container: `talia`, UID/GID 65532, private internal port 8080, read-only root,
  512 MiB memory ceiling, dropped capabilities and no host port publication.
- State: `/home/lelloman/homelab-data/talia/talia.sqlite3`.
- Credentials: `/home/lelloman/homelab-secrets/talia` (0700, files 0600).

The live homelab checkout had unrelated modifications. Only Talìa's additions were
merged into captured live files, with baseline hashes checked before writing.
Caddy and Knot configuration passed validation before reload. Original affected
files and image identities are retained at
`/home/lelloman/homelab-deployment-records/talia-20260921`.

## Access

Use the deployment key to sign in to the web dashboard. A private workstation
copy is at `.local/deployment/access.token`; the separate agent/alert operator key
is at `.local/deployment/operator.token`. These paths are gitignored and mode 0600.
The key is not a LelloAuth account; this first deployment gives trusted operator
access. Credential rotation and permissions are described in [README.md](README.md).

The welcome dashboard contains no sample metrics. The separate diagnostic variable
`deployment-check` records a persistence check and is not a monitoring signal.
Use MCP to author sources and dashboards. The workstation MCP adapter now accepts
verified HTTPS origins as well as loopback HTTP, while still rejecting remote
plaintext, URL credentials, query strings and paths. Redirects remain disabled.

```sh
/tmp/talia-p3-target/debug/talia-mcp https://talia.lan.lelloman.com .local/deployment/operator.token
```

The deployed image's adapter can also run over SSH with its loopback endpoint:

```sh
ssh homelab docker exec -i talia talia-mcp http://127.0.0.1:8080 /run/talia/operator.token
```

## Verification and recovery

[Machine evidence](first-deployment.json) records the completed live checks:
trusted HTTPS, browser sign-in and rendering, real stdio MCP over HTTPS, authored
state and writes, restart recovery, retained browser registration, unauthenticated
access rejection, health-monitor integration and the private proxy allowlist.
No certificate-validation bypass was used for live checks.

A consistent SQLite backup was made using SQLite's backup API, integrity checked,
and copied to an independent workstation. The diagnostic record exists in that
copy. Locations:

- Server: `/home/lelloman/homelab-data/talia/backups/first-deployment.sqlite3`.
- Workstation: `.local/deployment/first-deployment.sqlite3` (private, gitignored).

The evidence records its SHA-256. Private credential copies are retained separately.
This verified initial recovery set is not a claim that ongoing NAS backup has
been configured or proven for Talìa.

First-install rollback is stopping only Talìa and removing only its Caddy/DNS
additions after validation; retain state and credentials. Restart recovery was
exercised. Unrelated services and existing monitoring remained active. Before
future upgrades take a new consistent backup and retain the current image digest;
image rollback requires database schema compatibility. Do not restore stale state
over accepted alert or external-provider effects.

## Remaining scope

This is a first server/web deployment, not completed production qualification or
homelab monitoring migration. Prometheus, Grafana, Alertmanager and health-monitor
continue their existing duties. Real SMTP/Telegram/FCM credentials/delivery,
remote Android onboarding, LelloAuth sign-in and ongoing recovery operations remain
follow-up work. No physical Android device was used and no real alert messages
were dispatched as part of verification. Broader P5 acceptance remains open.
