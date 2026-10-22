#!/usr/bin/env python3
"""Fixture check for the LogErrorBurst retirement helper; never touches Talia."""
import copy
import importlib.util
import pathlib
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('retire_logerrorburst',
                                              HERE / 'retire-logerrorburst.py')
retire = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retire)


class Fixture:
    def __init__(self, metric=retire.METRIC):
        self.revision = 7
        self.writes = []
        self.config = {
            'policies': [],
            'destinations': [],
            'devices': [],
            'bindings': [
                {'id': ident, 'version': 1, 'policy': 'infra-warning',
                 'key': ident, 'params': {'name': 'LogErrorBurst'},
                 'inputs': {'result': retire.COLLECTOR}, 'labels': {},
                 'every_ms': 15000, 'enabled': True}
                for ident in retire.BINDINGS
            ],
        }
        self.catalog = {
            ('variable', retire.COLLECTOR): {'id': retire.COLLECTOR},
            ('monitor_instance', retire.COLLECTOR): {
                'id': retire.COLLECTOR,
                'params': {'query': metric},
            },
            ('monitor_instance', 'infra-ServiceDown'): {
                'id': 'infra-ServiceDown',
                'params': {'query': 'up == 0'},
            },
        }

    def tool(self, name, args):
        if name == 'alerts_config':
            return copy.deepcopy(self.config)
        if name == 'alerts_binding_save':
            expected, binding = args['expected'], args['binding']
            current = next(b for b in self.config['bindings'] if b['id'] == binding['id'])
            assert current['version'] == expected
            assert binding['version'] == expected + 1
            current.update(copy.deepcopy(binding))
            self.writes.append(name)
            return {'version': binding['version']}
        if name == 'definitions_list':
            kind = args['kind']
            records = [
                {'key': {'kind': k, 'id': ident}, 'revision': 1}
                for k, ident in self.catalog if k == kind
            ]
            return {'catalogRevision': self.revision, 'records': records,
                    'nextCursor': None}
        if name == 'definitions_read':
            records = []
            for key in args['keys']:
                document = self.catalog.get((key['kind'], key['id']))
                if document is None:
                    raise RuntimeError({'error': 'not_found'})
                records.append({'key': key, 'document': copy.deepcopy(document)})
            return {'catalogRevision': self.revision, 'records': records}
        if name == 'definitions_validate':
            return {'valid': True}
        if name == 'definitions_save':
            change_set = args['changeSet']
            assert change_set['expectedCatalogRevision'] == self.revision
            for change in change_set['changes']:
                assert change['op'] == 'delete'
                self.catalog.pop((change['key']['kind'], change['key']['id']))
            self.revision += 1
            self.writes.append(name)
            return {'audit': {'status': 'complete'}}
        raise AssertionError(name)


def enabled(fixture):
    return [b['id'] for b in fixture.config['bindings'] if b['enabled']]


with tempfile.TemporaryDirectory(prefix='retire-logerrorburst-') as temp:
    temp = pathlib.Path(temp)
    fixture = Fixture()
    retire.run(fixture, temp / 'preview', False)
    assert enabled(fixture) == list(retire.BINDINGS), 'preview must not write'
    assert ('monitor_instance', retire.COLLECTOR) in fixture.catalog
    assert not fixture.writes, 'preview must not call a write tool'

    retire.run(fixture, temp / 'apply', True)
    assert not enabled(fixture), 'bindings must be disabled'
    assert all(b['version'] == 2 for b in fixture.config['bindings'])
    assert ('variable', retire.COLLECTOR) not in fixture.catalog
    assert ('monitor_instance', retire.COLLECTOR) not in fixture.catalog
    assert ('monitor_instance', 'infra-ServiceDown') in fixture.catalog
    assert fixture.writes == ['alerts_binding_save', 'alerts_binding_save',
                              'definitions_save'], fixture.writes

    writes = list(fixture.writes)
    retire.run(fixture, temp / 'repeat', True)
    assert fixture.writes == writes, 'repeat apply must be a no-op'
    assert not enabled(fixture)

    unsafe = Fixture(metric='some_other_metric')
    try:
        retire.run(unsafe, temp / 'unsafe', True)
    except RuntimeError as error:
        assert 'refusing to delete' in str(error)
    else:
        raise AssertionError('mismatched collector must be refused')
    assert enabled(unsafe) == list(retire.BINDINGS), 'guard must not disable first'
    assert ('monitor_instance', retire.COLLECTOR) in unsafe.catalog

print('LogErrorBurst retirement checks passed: preview, apply, readback, repeat and guard.')
