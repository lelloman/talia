#!/usr/bin/env python3
"""Preview/apply rules.json query changes to existing collectors only.

Policies, bindings, acknowledgements and delivery settings are untouched; only
`infra-<rule>` monitor instances whose query differs from rules.json are saved.
"""
import argparse
import json
import pathlib
import uuid
from deploy import MCP, changes

ROOT = pathlib.Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backup', type=pathlib.Path, required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    args.backup.mkdir(mode=0o700, parents=True, exist_ok=False)

    def save(name, value):
        (args.backup / name).write_text(json.dumps(value, indent=2) + '\n')

    rules = json.loads((ROOT / 'rules.json').read_text())
    wanted = {c['key']['id']: c for c in changes(rules) if c['key']['kind'] == 'monitor_instance'}
    client = MCP()
    try:
        keys = [c['key'] for c in wanted.values()]
        records, revisions = [], set()
        for start in range(0, len(keys), 32):
            read = client.tool('definitions_read', {'keys': keys[start:start + 32]})
            revisions.add(read['catalogRevision'])
            records.extend(read['records'])
        if len(revisions) != 1:
            raise RuntimeError('Catalog changed during backup; reread before publishing')
        revision = revisions.pop()
        save('before.json', records)
        pending = []
        for record in records:
            old = record.get('document')
            if old is None:
                raise RuntimeError('Collector is not published; use deploy.py: ' + record['key']['id'])
            params = wanted[record['key']['id']]['document']['params']
            if old['params'] != params:
                document = {**old, 'params': params}
                if 'version' in old:
                    document['version'] = old['version'] + 1
                pending.append(dict(op='put', key=record['key'], document=document))
        change_set = dict(expectedCatalogRevision=revision, changes=pending)
        save('plan.json', change_set)
        print(f'{len(pending)} collector queries differ: ' + ', '.join(c['key']['id'] for c in pending)
              + f'; apply={args.apply}', flush=True)
        if not pending:
            return
        valid = client.tool('definitions_validate', {'changeSet': change_set})
        save('validation.json', valid)
        if not valid['valid']:
            raise RuntimeError(valid)
        if not args.apply:
            return
        save('result.json', client.tool('definitions_save', {'changeSet': change_set,
                                                             'requestId': 'query-update-' + uuid.uuid4().hex}))
        after = client.tool('definitions_read', {'keys': [c['key'] for c in pending]})
        save('after.json', after)
        for change, record in zip(pending, after['records']):
            assert record['key'] == change['key'] and record['document'] == change['document'], \
                'Readback differs from the requested change'
        print('Collector queries applied and read back successfully', flush=True)
    finally:
        client.close()


if __name__ == '__main__':
    main()
