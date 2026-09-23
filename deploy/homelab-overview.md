# Homelab Overview

The current dashboard presentation is maintained in [monitor refinement](monitor-refinement.md).
Apply that UI update after this initial collection setup.

[The authored changes](homelab-overview.json) contain the datasource, stored sample,
30-second collection Pipeline, reusable metric card and private `monitor` dashboard
published on 2026-09-22 through the signed-in administrator's temporary MCP key.
Prometheus at `http://prometheus:9090` supplies current CPU/memory usage, one hour
of five-minute trend samples, filesystem availability and service scrape status.
Both containers belong to homelab's `reverse-proxy-net`. No credentials or initial
sample values are embedded. Missing host readings remain unavailable; stale
collections retain their timestamp and explicitly show their quality.

To apply, read the current catalog, back up affected definitions, and pass this
array as `changeSet.changes` with the current `expectedCatalogRevision` to
`definitions_validate`. Save the validated set with `definitions_save` and a new
stable `requestId`. This replaces `monitor`; review its ownership, users and
current contents first. Applying the file does not change dashboard sharing.
Existing clients must reload the saved package.

The deployment-wide dashboard grant ceiling must permit only the required read:

```json
[{"family":"engine","actions":["read"],"scope":{"kind":"resource","id":"homelab-summary"}}]
```

On the initial deployment the ceiling was empty. With the user's approval,
`talia-agent --policy-only` added this entry during a brief Talìa-only stop/start;
operator grants were retained, policy version advanced from 1 to 2, and no new
credential was issued. See [offline policy provisioning](../docs/mcp-authoring.md).
Read and preserve existing policy/ceiling entries when repeating this operation;
do not overwrite unrelated permissions with this example.

## Verification and recovery

The server accepted all seven changes without diagnostics at catalog revision 8,
package `catalog-8-monitor`. Two consecutive scheduled runs completed with good
quality, three filesystems and 13 reachable scrape targets. This is scrape
reachability, not application health or completed homelab monitoring migration.
Shared UI compilation and ViewModel checks covered fresh, stale and unavailable
samples; desktop and mobile browser previews were inspected.

Both SQLite databases were backed up and integrity checked before the policy
change. Private backups and the old definitions, applied policy, unchanged image
ID and verification record are retained on homelab under
`/home/lelloman/homelab-deployment-records/talia-summary-permission-20260922T080906Z`.
To revert the dashboard, save the previous definition against the current catalog
revision. Before removing its deployment read permission, remove the dashboard's
read grant. Revert policy via offline provisioning using the then-current policy
version; preserve newer data instead of restoring an old database wholesale.

At initial verification, HTTP MCP engine/live operations were rejected because
the adapter's connection ID did not meet their 64-hex-character contract. Saved
authoring succeeded; collection was independently verified through read-only server
diagnostics. No remote browser reload was claimed. [TALIA-65](https://crumbles.lelloman.com/w/LLPR/TALIA/65)
corrects the adapter and adds real HTTP SDK engine/live regression coverage; deploy
that fix to enable remote inspection and reload.
