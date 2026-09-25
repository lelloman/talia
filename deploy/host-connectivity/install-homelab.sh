#!/bin/sh
set -eu
command -v python3
command -v curl
command -v getent
command -v crontab
install -d -m 755 /home/lelloman/homelab-data/report-probes
install -d -m 755 /home/lelloman/.local/bin
install -m 755 /tmp/talia-connectivity-probe.py /home/lelloman/.local/bin/talia-connectivity-probe
python3 /tmp/talia-configure-homelab.py /home/lelloman/homelab/monitoring/docker-compose.yml
/usr/bin/python3 /home/lelloman/.local/bin/talia-connectivity-probe --host Homelab --output /home/lelloman/homelab-data/report-probes/connectivity.prom
python3 - <<'PY'
import subprocess
from pathlib import Path
r=subprocess.run(['crontab','-l'],capture_output=True,text=True)
if r.returncode and 'no crontab' not in r.stderr: raise SystemExit(r.stderr)
old=r.stdout
backup=Path('/home/lelloman/homelab-data/report-probes/crontab.before-connectivity')
if not backup.exists(): backup.write_text(old);backup.chmod(0o600)
line='* * * * * /usr/bin/flock -n /home/lelloman/homelab-data/report-probes/probe.lock /usr/bin/python3 /home/lelloman/.local/bin/talia-connectivity-probe --host Homelab --output /home/lelloman/homelab-data/report-probes/connectivity.prom # talia-connectivity-probe'
lines=[s for s in old.splitlines() if not s.endswith('# talia-connectivity-probe')]
subprocess.run(['crontab','-'],input='\n'.join(lines+[line])+'\n',text=True,check=True)
PY
docker compose -f /home/lelloman/homelab/monitoring/docker-compose.yml up -d --no-deps node-exporter
cat /home/lelloman/homelab-data/report-probes/connectivity.prom
