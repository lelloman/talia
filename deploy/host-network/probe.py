#!/usr/bin/env python3
"""Publish physical host network counters into node_exporter's textfile directory."""
import argparse, os, pathlib, re, tempfile, time

def collect(interface, root=pathlib.Path('/sys/class/net'), now=time.time):
    if not re.fullmatch(r'[A-Za-z0-9_.:-]+', interface):
        raise ValueError('invalid interface')
    stats = root / interface / 'statistics'
    values = {direction: int((stats / filename).read_text()) for direction, filename in [('receive', 'rx_bytes'), ('transmit', 'tx_bytes')]}
    if any(value < 0 for value in values.values()):
        raise ValueError('negative counter')
    lines = []
    for direction, value in values.items():
        metric = 'talia_host_network_' + direction + '_bytes_total'
        lines += ['# TYPE ' + metric + ' counter', f'{metric}{{device="{interface}"}} {value}']
    lines += ['# TYPE talia_host_network_checked_timestamp_seconds gauge', 'talia_host_network_checked_timestamp_seconds ' + str(now())]
    return '\n'.join(lines) + '\n'

def publish(output, interface, root=pathlib.Path('/sys/class/net')):
    # On failure remove stale counters: unavailable must not look like zero traffic.
    try:
        data = collect(interface, root)
    except (OSError, ValueError):
        output.unlink(missing_ok=True)
        raise
    descriptor, temporary = tempfile.mkstemp(prefix='.host-network-', dir=output.parent)
    try:
        with os.fdopen(descriptor, 'w') as stream:
            stream.write(data)
        os.chmod(temporary, 0o644)
        os.replace(temporary, output)
    finally:
        pathlib.Path(temporary).unlink(missing_ok=True)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('interface')
    parser.add_argument('output', type=pathlib.Path)
    parser.add_argument('--twice', action='store_true', help='Collect now and in 30 seconds for a per-minute cron job')
    args = parser.parse_args()
    publish(args.output, args.interface)
    if args.twice:
        time.sleep(30)
        publish(args.output, args.interface)
