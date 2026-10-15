#!/usr/bin/env python3
"""Preview/apply the checklist steps of the infra reports from homelab-telegram-reports.json.

Only the reachability, availability and facts steps are replaced. Schedules,
destinations, the composer and every other step keep their live values.
"""
import argparse
import json
import pathlib
import sys
import uuid

ROOT = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / 'homelab-monitoring'))
from deploy import MCP  # noqa: E402

STEPS = ('reachability', 'availability', 'facts')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backup', type=pathlib.Path, required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    args.backup.mkdir(mode=0o700, parents=True, exist_ok=False)

    def save(name, value):
        (args.backup / name).write_text(json.dumps(value, indent=2) + '\n')

    repo = json.loads((ROOT / 'homelab-telegram-reports.json').read_text())
    client = MCP()
    try:
        plan = []
        for wanted in repo:
            live = client.tool('reports_get', {'id': wanted['id']})
            live = live.get('definition', live)
            save(wanted['id'] + '-before.json', live)
            if live['version'] != wanted['version'] - 1:
                raise RuntimeError(f"{wanted['id']}: live version {live['version']} is not the one this edit is based on")
            source = {s['id']: s for s in wanted['steps']}
            steps = [source[s['id']] if s['id'] in STEPS else s for s in live['steps']]
            changed = [a['id'] for a, b in zip(steps, live['steps']) if a != b]
            definition = {**live, 'steps': steps, 'version': live['version'] + 1}
            plan.append(dict(definition=definition, expected=live['version'],
                             requestId='report-checks-' + uuid.uuid4().hex))
            print(f"{wanted['id']}: v{live['version']} -> v{live['version'] + 1}, changed steps {changed}", flush=True)
        save('plan.json', plan)
        if not args.apply:
            return
        for request in plan:
            save(request['definition']['id'] + '-result.json', client.tool('reports_save', request))
            after = client.tool('reports_get', {'id': request['definition']['id']})
            after = after.get('definition', after)
            save(request['definition']['id'] + '-after.json', after)
            assert after == request['definition'], 'Readback differs from the requested change'
        print('Report checks applied and read back successfully', flush=True)
    finally:
        client.close()


if __name__ == '__main__':
    main()
