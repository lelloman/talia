#!/usr/bin/env python3
"""Publish native Talìa collection/policies through its guarded MCP API.

Default is shadow operation (no actions). --enable-delivery is the cutover step.
The existing approved Telegram destination is resolved at runtime, never copied
into checked-in definitions. Backups contain configuration, not credentials.
"""
import argparse
import json
import pathlib
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parent


class MCP:
    def __init__(self):
        self.proc = subprocess.Popen(['ssh', '-o', 'ConnectTimeout=10', '-o', 'ServerAliveInterval=15',
            '-o', 'ServerAliveCountMax=2', 'homelab', 'docker', 'exec', '-i', 'talia',
            'talia-mcp', 'http://127.0.0.1:8080', '/run/talia/operator.token'],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.sequence = 0
        self.rpc('initialize', dict(protocolVersion='2025-11-25', capabilities={},
            clientInfo=dict(name='homelab-migration', version='1')))
        self.proc.stdin.write(json.dumps(dict(jsonrpc='2.0', method='notifications/initialized'))+'\n')
        self.proc.stdin.flush()

    def rpc(self, method, params):
        self.sequence += 1
        self.proc.stdin.write(json.dumps(dict(jsonrpc='2.0', id=self.sequence, method=method, params=params))+'\n')
        self.proc.stdin.flush()
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError('MCP connection closed; inspect request status before retrying writes')
            reply = json.loads(line)
            if reply.get('id') == self.sequence:
                if 'error' in reply:
                    raise RuntimeError(reply['error'])
                return reply['result']

    def tool(self, name, args):
        result = self.rpc('tools/call', dict(name=name, arguments=args))
        value = result.get('structuredContent')
        if result.get('isError') or value.get('error'):
            raise RuntimeError(value)
        return value

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=10)


def encode(value):
    def node(v):
        if v is None: return ['null']
        if isinstance(v, bool): return ['boolean', v]
        if isinstance(v, str): return ['string', v]
        if isinstance(v, (int, float)): return ['number', v]
        if isinstance(v, list): return ['array', [node(x) for x in v]]
        return ['object', [[k, node(v[k])] for k in sorted(v)]]
    return dict(version=1, value=node(value))


def changes(rules):
    out = []
    def put(kind, ident, document):
        out.append(dict(op='put', key=dict(kind=kind, id=ident), document=document))
    put('variable_definition', 'infra-alert-result', dict(id='infra-alert-result', version=1,
        kind='stored', source='', value_schema='any', state_schema='any', dependencies=[]))
    put('monitor_definition', 'infra-alert-query', dict(id='infra-alert-query', version=1,
        kind='pipeline', source=(ROOT/'collect.js').read_text()))
    for rule in rules:
        ident = 'infra-'+rule['name']
        put('variable', ident, dict(id=ident, definition='infra-alert-result', params=encode({}),
            history_count=120, history_age_ms=7200000))
        put('monitor_instance', ident, dict(id=ident, definition='infra-alert-query',
            sources={'prom':'homelab-prometheus'}, outputs={'result':ident},
            params=encode({'query':rule['query']}), schedule=dict(kind='interval', every_ms=rule['intervalMs']),
            stale_after_ms=max(90000, rule['intervalMs']*3), timeout_ms=15000))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--enable-delivery', action='store_true')
    ap.add_argument('--update-definitions', action='store_true')
    ap.add_argument('--backup', type=pathlib.Path, required=True)
    args = ap.parse_args()
    args.backup.mkdir(parents=True, exist_ok=False, mode=0o700)
    rules = json.loads((ROOT/'rules.json').read_text())
    m = MCP()
    def save(name, data):
        (args.backup/name).write_text(json.dumps(data, indent=2)+'\n')
    def mutate(name, value):
        request = {**value, 'requestId':'infra-migration-'+uuid.uuid4().hex}
        save(request['requestId']+'.json', dict(name=name, arguments=request))
        return m.tool(name, request)
    try:
        config = m.tool('alerts_config', {})
        save('alerts-before.json', config)
        destinations = [d['id'] for d in config['destinations'] if d['channel']=='telegram' and d['enabled']]
        if len(destinations)!=1: raise RuntimeError('Expected one existing approved Telegram destination')
        wanted = changes(rules)
        records = []
        keys = []
        cursor = None
        revision = None
        while True:
            page = m.tool('definitions_list', {'limit':100, **({'cursor':cursor} if cursor else {})})
            if revision is not None and revision != page['catalogRevision']:
                raise RuntimeError('Catalog changed during inventory; reread before publishing')
            revision = page['catalogRevision']
            keys.extend(x['key'] for x in page['records'])
            cursor = page.get('nextCursor')
            if not cursor: break
        present = [x['key'] for x in wanted if x['key'] in keys]
        for start in range(0,len(present),32):
            read = m.tool('definitions_read', {'keys':present[start:start+32]})
            if read['catalogRevision'] != revision:
                raise RuntimeError('Catalog changed during backup; reread before publishing')
            records.extend(read['records'])
        save('definitions-before.json', records)
        existing = {(x['key']['kind'], x['key']['id']):x for x in records}
        pending = []
        for change in wanted:
            old = existing.get((change['key']['kind'], change['key']['id']))
            if not old or old.get('document') is None:
                pending.append(change)
            elif any(old['document'].get(k) != v for k,v in change['document'].items() if k != 'version'):
                if not args.update_definitions:
                    raise RuntimeError('Existing definition differs; reconcile explicitly: '+str(change['key']))
                change['document'] = {**old['document'], **change['document']}
                if 'version' in old['document']:
                    change['document']['version'] = old['document']['version']+1
                pending.append(change)
        if pending:
            cs = dict(expectedCatalogRevision=revision, changes=pending)
            valid = m.tool('definitions_validate', {'changeSet':cs})
            save('validation.json', valid)
            if not valid['valid']: raise RuntimeError(valid)
            save('catalog-save.json', mutate('definitions_save', {'changeSet':cs}))
        for severity in ['warning','critical','availability']:
            ident = 'infra-'+severity
            source = (ROOT/('availability.js' if severity=='availability' else 'condition.js')).read_text()
            previous = next((p for p in config['policies'] if p['id']==ident), None)
            version = previous['version'] if previous else 0
            actions = []
            if args.enable_delivery:
                actions = [dict(id='telegram', destinations=destinations, delay_ms=0 if severity=='critical' else 30000,
                    repeat_ms=14400000 if severity=='critical' else 43200000, until_ack=True,
                    max_attempts=3, retry_ms=30000, expiry_ms=14400000)]
            policy = dict(id=ident, version=version+1, source=source,
                stages={s:dict(reset_ack=True, actions=actions) for s in ['firing-a','firing-b']},
                recovery=[dict(id='recovered', destinations=destinations, max_attempts=3, retry_ms=30000)] if args.enable_delivery else [])
            mutate('alerts_policy_save', dict(policy=policy, expected=version))
        for rule in rules:
            ident = 'infra-'+rule['name']
            params = dict(name=rule['name'], forMs=rule['forMs'], staleMs=max(90000, rule['intervalMs']*3),
                severity=rule['labels']['severity'], summary=rule['annotations']['summary'], description=rule['annotations']['description'])
            if rule['name'] == 'SSHAuthenticationFailureBurst':
                params['notify'] = False
            for guard in [False, True]:
                bid = ident+('-input' if guard else '')
                previous = next((b for b in config['bindings'] if b['id']==bid), None)
                if previous: continue  # Preserve pending timers, acknowledgement and live overrides.
                binding = dict(id=bid, version=1, policy='infra-'+('availability' if guard else params['severity']),
                    key=bid, params=params, inputs={'result':ident}, labels={**rule['labels'], 'alertname':rule['name'], 'migration':'talia'},
                    every_ms=min(15000,rule['intervalMs']), enabled=True)
                mutate('alerts_binding_save', dict(binding=binding, expected=0))
        save('alerts-after.json', m.tool('alerts_config', {}))
        save('snapshot.json', m.tool('alerts_snapshot', {}))
        print(f'{len(rules)} native conditions and {len(rules)} input availability guards configured; delivery={args.enable_delivery}', flush=True)
    finally:
        m.close()


if __name__ == '__main__': main()
