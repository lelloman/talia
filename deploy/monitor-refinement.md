# Monitor dashboard refinement

The [catalog changes](monitor-refinement.json) replace only the saved `monitor`
dashboard and its reusable metric card. The source lives in
[`dashboard/examples/homelab`](../dashboard/examples/homelab/monitor.ui).
Collection, datasource permissions, dashboard sharing and ownership are unchanged.

The layout uses prominent CPU/memory readings with compact one-hour trends,
responsive disk and service cards, explicit collection quality and timestamps,
and unreachable services first. Colors supplement readable status labels.
Scrape reachability is not full application health. Threshold colors are dashboard
presentation (90% CPU/memory, under 10% disk free), not alert rules.

Deploy the updated server/web renderer first. Android clients also need a new
build before loading this package: older hosts reject the new semantic properties.
Read and back up the current affected definitions, compare their grants and
references, then call `definitions_validate` with these changes and the current
`expectedCatalogRevision`. Save with `definitions_save` and a unique `requestId`.
Clients adopt the new saved package on explicit Reload; saving never discards live
edits. To roll back, save the backed-up definitions against the latest catalog.

Run `node dashboard/tests/homelab.mjs` for real-browser desktop/mobile and dark-mode
previews, including unavailable readings, degraded services and stale collections.
Screenshots are written beneath `.local/monitor-redesign/`.
