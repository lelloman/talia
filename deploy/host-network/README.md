# Host network dashboard

The shared host collector uses a configured `networkDevice` to avoid counting
Docker bridges, veth peers and tunnel traffic in addition to physical traffic.
The initial devices are Homelab `enp3s0` and VPS-EU/VPS-US `ens6`. This is traffic
on that host interface, including LAN traffic, rather than Internet billing.

VPSes use native `node_network_receive_bytes_total` and
`node_network_transmit_bytes_total`. Homelab's node-exporter runs in Docker's
network namespace and exposes container `eth0`, so `probe.py` reads the host's
physical interface counters from sysfs into its existing textfile directory.
`install-homelab.py` preserves the crontab and installs an independent flocked
probe every 30 seconds, with no changes to exporter networking or other probes.
Copy `probe.py` to `/tmp/talia-network-probe.py`, then run the installer as the
existing homelab user. Collector failures remove stale textfile metrics. A
90-second timestamp check masks current readings if collection stops.

The graph covers 60 minutes at 30-second spacing, using `irate` (bytes/second
between the latest two scrapes). Both directions use a shared dynamic scale in
B/s, KB/s, MB/s or GB/s, with matching line legends. Historical gaps stay gaps.
Current rates require a reachable exporter and fresh collection. Network source
failures are isolated from CPU, memory, disk and repository collection.

Transfer totals use `sum(increase(counter[1d|7d|30d]))`, resetting each interface
counter before aggregation. They are Prometheus estimates, not billing totals.
The minimum retained sample count across both directions estimates available
history at the configured 30-second scrape interval. Under 99.5% coverage or
source warnings is marked partial; missing history is unavailable, not zero.
Prometheus currently retains 30 days. Newly introduced Homelab counters cannot
reconstruct earlier host traffic, and its first totals are partial.

Tests: `python3 deploy/host-network/test_probe.py`,
`node dashboard/tests/host-template.mjs`, and
`node dashboard/tests/host-browser.mjs`. The shared Chart contract supports
`secondaryValues`, `primaryLabel` and `secondaryLabel`; existing `peakValues`
CPU charts keep their default Average/Max labels. Deploy the updated validator
and renderers before publishing the network layout.

To stop host collection, remove only the crontab line ending
`# talia-host-network-probe` and the `host-network.prom` textfile. To undo the
layout, restore only the saved shared definitions and affected host-instance
network parameters. Preserve other live parameters, grants and state.
