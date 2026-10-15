#!/usr/bin/env python3
"""Preview/apply notification-only changes without republishing collectors."""
import argparse
import json
import pathlib
import uuid
from deploy import MCP

ROOT = pathlib.Path(__file__).resolve().parent
QUIET_CONDITIONS = ('SimpleAgentsCapacityExhausted', 'LogErrorBurst')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backup', type=pathlib.Path, required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    args.backup.mkdir(mode=0o700, parents=True, exist_ok=False)

    def save(name, value):
        (args.backup / name).write_text(json.dumps(value, indent=2) + '\n')

    client = MCP()
    try:
        config = client.tool('alerts_config', {})
        save('before.json', config)
        policy = next(p for p in config['policies'] if p['id'] == 'infra-warning')
        source = (ROOT / 'condition.js').read_text()
        changes = []
        if policy['source'] != source:
            changes.append(('alerts_policy_save', dict(
                policy={**policy, 'version': policy['version'] + 1, 'source': source},
                expected=policy['version'])))
        for name in (*QUIET_CONDITIONS, 'SimpleAgentsSessionNoProgress'):
            binding = next(b for b in config['bindings'] if b['id'] == 'infra-' + name)
            assert binding['policy'] == 'infra-warning', 'Unexpected policy binding'
            params = dict(binding['params'])
            if name in QUIET_CONDITIONS:
                params['notify'] = False
            else:
                params['quietStates'] = ['queued']
            if params != binding['params']:
                changes.append(('alerts_binding_save', dict(
                    binding={**binding, 'version': binding['version'] + 1, 'params': params},
                    expected=binding['version'])))
        requests = [dict(tool=tool, arguments={**arguments, 'requestId':
                    'notification-tuning-' + uuid.uuid4().hex}) for tool, arguments in changes]
        save('plan.json', requests)
        print(f'{len(requests)} guarded changes prepared; apply={args.apply}', flush=True)
        if not args.apply:
            return
        for index, request in enumerate(requests):
            result = client.tool(request['tool'], request['arguments'])
            save(f'result-{index}.json', result)
        after = client.tool('alerts_config', {})
        save('after.json', after)
        for request in requests:
            kind = 'policies' if request['tool'] == 'alerts_policy_save' else 'bindings'
            key = 'policy' if kind == 'policies' else 'binding'
            expected = request['arguments'][key]
            actual = next(x for x in after[kind] if x['id'] == expected['id'])
            assert actual == expected, 'Readback differs from the requested change'
        print('Notification changes applied and read back successfully', flush=True)
    finally:
        client.close()


if __name__ == '__main__':
    main()
