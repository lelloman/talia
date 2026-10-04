#!/usr/bin/env python3
"""Install alongside an existing node_exporter textfile directory, as its repo owner."""
import argparse
from pathlib import Path
import shutil
import subprocess
import shlex
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--config', required=True)
p.add_argument('--directory', required=True, help='Existing node_exporter textfile directory')
a = p.parse_args()
root = Path(a.directory).resolve()
if not root.is_dir():
    raise SystemExit('Configure node_exporter textfile collection first')
bin_dir = Path.home() / '.local/bin'
bin_dir.mkdir(parents=True, exist_ok=True)
config = root / 'git-workspaces-config.json'
script = bin_dir / 'talia-git-workspaces'
cron = subprocess.run(['crontab', '-l'], capture_output=True, text=True)
if cron.returncode and 'no crontab' not in cron.stderr:
    raise SystemExit(cron.stderr)
stamp = str(time.time_ns())
(root / ('crontab.before-git-' + stamp)).write_text(cron.stdout)
if config.exists():
    shutil.copyfile(config, str(config) + '.before-' + stamp)
shutil.copyfile(a.config, config)
shutil.copyfile(Path(__file__).with_name('probe.py'), script)
command = ['/usr/bin/flock', '-n', str(root / 'git-workspaces.lock'), '/usr/bin/python3', str(script), '--config', str(config), '--output', str(root / 'git-workspaces.prom')]
subprocess.run(command, check=True)
# Check the persisted deadline every minute; Git runs only once per elapsed 24h.
line = '* * * * * ' + shlex.join(command) + ' # talia-git-workspaces'
lines = [s for s in cron.stdout.splitlines() if not s.endswith('# talia-git-workspaces')]
subprocess.run(['crontab', '-'], input='\n'.join(lines + [line]) + '\n', text=True, check=True)
