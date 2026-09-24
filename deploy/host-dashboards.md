# Shared host dashboards

`node deploy/host-dashboards.mjs` emits catalog changes for `homelab`, `vps-eu`
and `vps-us`. The obsolete `monitor` dashboard was removed after initial publication. No server or
client binary change is required beyond the semantic renderer from TALIA-79.

Each dashboard references **host-layout** (UI) and **host-present** (formatting
function), with the same small subscription ViewModel. Its parameters are `host`
(display name) and `summary` (the exact server Variable to subscribe to). The UI
also references the shared **host-metric-card**. Edit these shared definitions
to change all three dashboards on their next explicit Reload. Changing one
instance's parameters affects only that dashboard. Live state is not auto-reset.

All three collection instances reference **host-collect**, with parameters `host`,
`job`, and `instance`. Each has its own `host-HOST` Variable, output mapping,
30-second schedule, 90-second stale threshold and one-hour retained snapshot history (separate from the 24-hour Prometheus chart window).
CPU, memory and filesystem selectors match the exact job and instance, including
range queries. The CPU card switches between a 24-hour view of five-minute CPU
averages (289 slots) and a one-hour view of one-minute CPU averages (61 slots).
Memory remains a 24-hour view with readings every five minutes. Missing samples
remain gaps at their true position. A down exporter clears current values; retained trends can still
show the earlier history. Newly added exporters need two scrapes for CPU rates
and accumulate the 24-hour chart naturally. Disk cards follow discovered mounts.
Host reachability means metrics collection works, not that every service is healthy.

CPU and memory charts use a fixed 0–100% scale with quarter-grid labels, 24-hour
endpoints, and a dashed 90% high-usage reference. The memory card also shows
absolute used, total, and available memory. Used is `MemTotal − MemAvailable`,
matching the percentage and excluding memory Linux considers readily reclaimable.
If those exporter series are missing, absolute usage stays unavailable rather than
showing a misleading zero.

Homelab uses the existing `node-exporter` job. VPSes use `node-exporter-vps`, keeping
existing homelab-only queries unchanged. Their distro exporters listen only on
127.0.0.1:9100. Monitoring-stack SSH sidecars forward that endpoint on the internal
Docker network, with no published host ports. Separate keys, verified pinned host
keys, forwarding-only remote accounts and SSH keepalives are configured in the
homelab repository under `monitoring/vps-metrics/`.

## Applying or extending

1. Ensure the new host's exporter is scraped and `up` is 1. Add its job/instance
   mapping to `hosts` in the generator, or author the corresponding records via MCP.
2. Back up affected catalog definitions and dashboard sharing. Read the current
   catalog revision. The generator is an initial desired configuration, not a
   blind reapply tool: preserve any newer parameters and permissions.
3. Permit only the new summary resource in the deployment dashboard read ceiling.
   Preserve the current operator grants and existing ceiling entries. This uses
   offline `talia-agent --policy-only` provisioning, with a database backup and a
   guarded Talìa restart when Telegram/report work is idle.
4. Validate the complete change set via `definitions_validate`; save with
   `definitions_save`, current `expectedCatalogRevision`, and a unique `requestId`.
   For subsequent template changes, put only the changed shared definition.
5. Assign each new dashboard to the intended admin using `dashboard_access_set`.
   These initial dashboards are private to the same owner as `monitor`.
6. Verify each `host-HOST` sample has good quality, its matching host and fresh
   CPU/memory/filesystem values. Refresh the web page to discover the dashboards.

For later metric-card revisions, save only the changed `host-metric-card` UI,
`host-present` function, and `host-collect` monitor definition with its incremented
version against the current
catalog revision. Deploy the updated web renderer before saving UI with chart
range properties. Running dashboards adopt the new saved package on explicit Reload;
the collector updates server samples on its next scheduled run.

Checks: `node dashboard/tests/host-template.mjs` covers selectors, independent
subscriptions and grants, layout resolution and down/stale/missing data;
`node dashboard/tests/host-browser.mjs` exercises all three at 390px/1280px.
Screenshots are in `.local/host-template/`.

Rollback: remove the three dashboard definitions first, then their collection
instances/Variables and unused shared definitions. Preserve the original overview.
Remove corresponding ceiling entries only after their dashboard grants are gone.
Remove the VPS scrape job and reload Prometheus, then stop the two tunnel services.
Disable the VPS exporters and remove their dedicated SSH accounts/keys if no other
monitoring uses them. Do not restore an old database over newer operational data.

## Initial publication

Published 2026-09-23 at catalog revision 15, with private ownership matching
`monitor`. All three collectors returned good, fresh samples with finite CPU and
memory values and three filesystems each. Both VPS scrape targets reported up.
The read ceiling advanced from operator policy version 2 to 3 without changing
operator grants or issuing a credential. Pre-change policy records are retained
on homelab in `homelab-deployment-records/talia-host-template-20260923`; the engine
backup is `/data/before-host-template.sqlite3` in the Talìa data volume.
