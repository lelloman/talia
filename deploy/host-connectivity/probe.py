#!/usr/bin/env python3
"""Local-origin connectivity checks, atomically exported through node_exporter."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

TARGETS = {
    'google': ('www.google.com', 'https://www.google.com/generate_204'),
    'cloudflare': ('www.cloudflare.com', 'https://www.cloudflare.com/cdn-cgi/trace'),
}

def check(kind, target):
    domain, url = TARGETS[target]
    cmd = ['getent', 'ahosts', domain] if kind == 'dns' else [
        'curl', '--noproxy', '*', '--fail', '--silent', '--show-error',
        '--max-time', '8', '--connect-timeout', '4', '--output', '/dev/null', url,
    ]
    try:
        r = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=10)
        return r.returncode == 0 and (kind != 'dns' or bool(r.stdout.strip()))
    except (OSError, subprocess.TimeoutExpired):
        return False

def render(host, results, checked):
    label = 'host=' + json.dumps(host)
    lines = []
    for kind in ('dns', 'https'):
        labels = label + ',kind=' + json.dumps(kind)
        lines.append(f'talia_report_probe_success{{{labels}}} {int(any(results[kind].values()))}')
        lines.append(f'talia_report_probe_checked_timestamp_seconds{{{labels}}} {checked:.3f}')
        for target, success in results[kind].items():
            lines.append(f'talia_report_probe_target_success{{{labels},target={json.dumps(target)}}} {int(success)}')
    return '\n'.join(lines) + '\n'

def atomic_write(path, text):
    # Temporary files do not end in .prom, so node_exporter cannot scrape half a run.
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, prefix='.connectivity-', delete=False) as f:
        tmp = f.name
        f.write(text)
        f.flush()
        os.fsync(f.fileno())
    try:
        os.chmod(tmp, 0o644)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--host', required=True, choices=['Homelab', 'VPS-EU', 'VPS-US'])
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    started = time.time()
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = {(kind, target): pool.submit(check, kind, target) for kind in ('dns', 'https') for target in TARGETS}
        results = {kind: {target: futures[kind, target].result() for target in TARGETS} for kind in ('dns', 'https')}
    atomic_write(args.output, render(args.host, results, started))

if __name__ == '__main__':
    main()
