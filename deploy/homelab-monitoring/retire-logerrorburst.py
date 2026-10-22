#!/usr/bin/env python3
"""Preview/apply retirement of the live LogErrorBurst bindings and collector.

The publisher only creates or preserves definitions for current rules, so
removing a rule from rules.json leaves its live bindings and collector behind.
This helper disables those two exact bindings, verifies they are inactive, then
deletes the collector variable and monitor instance. It refuses to delete a
collector that does not contain the retired metric and reads every change back.
"""
import argparse
import json
import pathlib
import uuid

from deploy import MCP

BINDINGS = ('infra-LogErrorBurst', 'infra-LogErrorBurst-input')
COLLECTOR = 'infra-LogErrorBurst'
METRIC = 'talia_health_log_errors_1h'
KEYS = (
    {'kind': 'variable', 'id': COLLECTOR},
    {'kind': 'monitor_instance', 'id': COLLECTOR},
)


def save(directory, name, value):
    (directory / name).write_text(json.dumps(value, indent=2) + '\n')


def catalog_keys(client):
    keys, revision = [], None
    for kind in ('variable', 'monitor_instance'):
        cursor = None
        while True:
            args = {'kind': kind, 'limit': 100}
            if cursor:
                args['cursor'] = cursor
            page = client.tool('definitions_list', args)
            if revision is not None and revision != page['catalogRevision']:
                raise RuntimeError('Catalog changed during inventory; reread before cleanup')
            revision = page['catalogRevision']
            keys.extend(record['key'] for record in page['records'])
            cursor = page.get('nextCursor')
            if not cursor:
                break
    return revision, keys


def run(client, backup, apply):
    backup.mkdir(parents=True, exist_ok=False, mode=0o700)
    config = client.tool('alerts_config', {})
    save(backup, 'alerts-before.json', config)

    bindings = {binding['id']: binding for binding in config['bindings']}
    disable = []
    for ident in BINDINGS:
        binding = bindings.get(ident)
        if binding is None:
            print(f'{ident}: not present', flush=True)
        elif not binding['enabled']:
            print(f'{ident}: already disabled at version {binding["version"]}', flush=True)
        else:
            disable.append(binding)

    revision, keys = catalog_keys(client)
    present = [key for key in KEYS if key in keys]
    records = client.tool('definitions_read', {'keys': present})['records'] if present else []
    save(backup, 'definitions-before.json', records)
    monitor = next(
        (record for record in records if record['key']['kind'] == 'monitor_instance'),
        None,
    )
    if monitor is not None and METRIC not in json.dumps(monitor['document']):
        raise RuntimeError(
            f'{COLLECTOR} collector does not contain {METRIC}; refusing to delete'
        )
    if present:
        print(f'{COLLECTOR}: collector keys present: '
              + ', '.join(key['kind'] for key in present), flush=True)
    else:
        print(f'{COLLECTOR}: collector already absent', flush=True)

    for binding in disable:
        print(f'{binding["id"]}: would disable version {binding["version"]}', flush=True)
    if present:
        print(f'{COLLECTOR}: would delete {len(present)} collector definitions', flush=True)
    if not apply:
        return

    for binding in disable:
        disabled = {**binding, 'version': binding['version'] + 1, 'enabled': False}
        request = {
            'binding': disabled,
            'expected': binding['version'],
            'requestId': 'retire-logerrorburst-' + uuid.uuid4().hex,
        }
        save(backup, request['requestId'] + '.json',
             {'name': 'alerts_binding_save', 'arguments': request})
        client.tool('alerts_binding_save', request)
        print(f'{binding["id"]}: disabled as version {disabled["version"]}', flush=True)

    after = client.tool('alerts_config', {})
    save(backup, 'alerts-after.json', after)
    remaining = {binding['id']: binding for binding in after['bindings']}
    for ident in BINDINGS:
        binding = remaining.get(ident)
        if binding is not None and binding['enabled']:
            raise RuntimeError(f'{ident} is still enabled after cleanup')

    if present:
        change_set = {
            'expectedCatalogRevision': revision,
            'changes': [{'op': 'delete', 'key': key} for key in present],
        }
        save(backup, 'plan.json', change_set)
        valid = client.tool('definitions_validate', {'changeSet': change_set})
        save(backup, 'validation.json', valid)
        if not valid['valid']:
            raise RuntimeError(valid)
        request = {'changeSet': change_set,
                   'requestId': 'retire-logerrorburst-' + uuid.uuid4().hex}
        save(backup, request['requestId'] + '.json',
             {'name': 'definitions_save', 'arguments': request})
        client.tool('definitions_save', request)
        _, keys = catalog_keys(client)
        if any(key in keys for key in present):
            raise RuntimeError('Collector definitions still present after cleanup')
        print(f'{COLLECTOR}: collector deleted and read back as absent', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backup', type=pathlib.Path, required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    client = MCP()
    try:
        run(client, args.backup, args.apply)
    finally:
        client.close()


if __name__ == '__main__':
    main()
