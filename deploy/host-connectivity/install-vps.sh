#!/bin/sh
set -eu
host=$1
case "$host" in VPS-EU|VPS-US) ;; *) exit 1 ;; esac
command -v python3
command -v curl
command -v getent
install -d -o prometheus -g prometheus -m 755 /var/lib/talia-report-probes
install -m 755 /tmp/talia-connectivity-probe.py /usr/local/bin/talia-connectivity-probe
cat > /etc/systemd/system/talia-connectivity-probe.service <<UNIT
[Unit]
Description=Local DNS and HTTPS connectivity measurements for Talia
After=network-online.target
[Service]
Type=oneshot
User=prometheus
Group=prometheus
ExecStart=/usr/bin/python3 /usr/local/bin/talia-connectivity-probe --host $host --output /var/lib/talia-report-probes/connectivity.prom
TimeoutStartSec=20
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/talia-report-probes
UNIT
cat > /etc/systemd/system/talia-connectivity-probe.timer <<'UNIT'
[Unit]
Description=Measure host connectivity every minute
[Timer]
OnBootSec=15s
OnUnitActiveSec=60s
AccuracySec=1s
[Install]
WantedBy=timers.target
UNIT
python3 - <<'PY'
from pathlib import Path
p = Path('/etc/systemd/system/prometheus-node-exporter.service.d/listen.conf')
s = p.read_text()
flag = '--collector.textfile.directory=/var/lib/talia-report-probes'
if flag not in s:
    if '--collector.textfile.directory' in s:
        raise SystemExit('Existing textfile directory requires manual reconciliation')
    backup = p.with_name('listen.conf.before-connectivity')
    if not backup.exists(): backup.write_text(s)
    s = s.replace('ExecStart=/usr/bin/prometheus-node-exporter --web.listen-address=127.0.0.1:9100', 'ExecStart=/usr/bin/prometheus-node-exporter --web.listen-address=127.0.0.1:9100 '+flag)
    if flag not in s: raise SystemExit('Unexpected exporter command')
    p.write_text(s)
PY
systemctl daemon-reload
systemctl start talia-connectivity-probe.service
systemctl enable --now talia-connectivity-probe.timer
systemctl restart prometheus-node-exporter
cat /var/lib/talia-report-probes/connectivity.prom
