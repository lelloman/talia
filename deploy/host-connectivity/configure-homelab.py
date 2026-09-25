#!/usr/bin/env python3
"""Add only the connectivity textfile directory to Homelab's existing exporter."""
from pathlib import Path
import sys
p=Path(sys.argv[1]);s=p.read_text()
start=s.index('  node-exporter:\n');end=s.index('\n  prometheus:',start)
section=s[start:end]
flag="      - '--collector.textfile.directory=/var/lib/talia-report-probes'\n"
mount='      - /home/lelloman/homelab-data/report-probes:/var/lib/talia-report-probes:ro\n'
if flag not in section:
    if '--collector.textfile.directory' in section: raise SystemExit('Existing textfile collector requires reconciliation')
    section=section.replace('    command:\n','    command:\n'+flag)
if mount not in section: section=section.replace('    volumes:\n','    volumes:\n'+mount)
assert flag in section and mount in section
if section!=s[start:end]:
    backup=p.with_name(p.name+'.before-connectivity')
    if not backup.exists(): backup.write_text(s)
    p.write_text(s[:start]+section+s[end:])
