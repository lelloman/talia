#!/usr/bin/env python3
"""Install the host counter probe without changing node_exporter or other cron jobs."""
import pathlib, shutil, subprocess, shlex
home=pathlib.Path.home(); directory=home/'homelab-data/report-probes'
if not (pathlib.Path('/sys/class/net/enp3s0/statistics/rx_bytes').is_file() and directory.is_dir()):
    raise SystemExit('Expected physical interface or existing textfile directory missing')
source=pathlib.Path('/tmp/talia-network-probe.py'); target=directory/'network-probe.py'
current=subprocess.run(['crontab','-l'],capture_output=True,text=True)
if current.returncode not in (0,1):raise SystemExit('Cannot read crontab')
if current.returncode==1 and current.stdout:raise SystemExit('Unexpected crontab failure')
records=home/'homelab-deployment-records/talia-network-20261007';records.mkdir(mode=0o700,parents=True,exist_ok=True)
backup=records/'crontab.before'
if not backup.exists():backup.write_text(current.stdout)
if target.exists() and not (records/'network-probe.before.py').exists():shutil.copy2(target,records/'network-probe.before.py')
shutil.copy2(source,target)
output=directory/'host-network.prom'
subprocess.run(['python3',str(target),'enp3s0',str(output)],check=True)
line='* * * * * /usr/bin/flock -n '+shlex.quote(str(directory/'network.lock'))+' /usr/bin/python3 '+shlex.quote(str(target))+' enp3s0 '+shlex.quote(str(output))+' --twice # talia-host-network-probe'
lines=[item for item in current.stdout.splitlines() if not item.endswith('# talia-host-network-probe')]
subprocess.run(['crontab','-'],input='\n'.join(lines+[line])+'\n',text=True,check=True)
print('Installed physical interface counter probe for enp3s0 every 30 seconds')
