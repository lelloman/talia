# Per-host connectivity checks

`probe.py` runs on each host using its normal DNS configuration and network
namespace. It resolves `www.google.com` and `www.cloudflare.com`, and makes
certificate-verified HTTPS GETs to Google's `generate_204` and Cloudflare's
`cdn-cgi/trace`. HTTPS bodies are discarded and proxies are bypassed. Each kind
passes when at least one of its two independent endpoints succeeds. Individual
endpoint results are retained too; this is limited external-connectivity evidence,
not a claim that every Internet destination is reachable.

Each run performs four bounded checks concurrently, then atomically replaces the
textfile result. Failure writes zero instead of retaining success. Each kind has
an actual collection timestamp; report logic rejects missing, >120-second-old,
or implausibly future timestamps even when node_exporter keeps scraping an old file.

## Deployment

VPS-EU and VPS-US run a systemd oneshot as the existing `prometheus` user, scheduled
every 60 seconds. Results live in `/var/lib/talia-report-probes`, read through
node_exporter's textfile collector. Exporters retain their loopback-only bind and
existing SSH tunnels. No new listener or credential is created.

Copy `probe.py` to `/tmp/talia-connectivity-probe.py` on a VPS, then run
`install-vps.sh VPS-EU` (or `VPS-US`) as root. The installer preserves a backup of
the previous exporter override and refuses to replace an existing textfile path.

Homelab runs via lelloman's crontab with `flock`, independent of interactive login
and user-systemd lingering. Results live in `~/homelab-data/report-probes`.
Copy `probe.py` and `configure-homelab.py` to `/tmp/talia-connectivity-probe.py`
and `/tmp/talia-configure-homelab.py`, then run `install-homelab.sh` as lelloman.
Only node-exporter is recreated. Its Compose service gets a read-only bind of the
result directory and a textfile collector flag. The existing crontab and Compose
file are backed up, and unrelated configuration is preserved. The same Compose
change must be maintained in the local Homelab repository.

Check `talia_report_probe_success` and
`talia_report_probe_checked_timestamp_seconds` in Prometheus: six series each,
one per host/kind. Do not apply `timestamp()` to a union of metric names with
identical labels: Prometheus drops the name and rejects the duplicate label sets.
The report checks the exported collection timestamp instead.

## Verification

`python3 deploy/host-connectivity/test_probe.py` checks failure/timeout handling,
endpoint aggregation and atomic replacement. `node deploy/test-host-reports.cjs`
checks stale-success rejection and real per-host HTTPS failure rendering.
All three deployed collectors returned DNS=1 and HTTPS=1 on 2026-09-25. The
unsent report `report-1184dfd037f90566588a18ff7ec8a197` evaluated all three hosts as
OK with no host problems. Other checklist coverage gaps are independent.

## Rollback

Disable/remove the dedicated VPS timer/service and restore the saved exporter
`listen.conf.before-connectivity`, then daemon-reload/restart the exporter.
On Homelab, remove only the crontab line ending `# talia-connectivity-probe` and
the added exporter mount/flag, preserving subsequent unrelated edits. Recreate
only node-exporter. Probe files can remain inert; reports will correctly warn
when their collection timestamps become stale.
