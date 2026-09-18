#!/usr/bin/env python3
"""Validate recorded evidence parity, not full runtime qualification."""
import json
from pathlib import Path
root = Path(__file__).parent / 'results'
reports = {name: json.loads((root / f'{name}.json').read_text())
           for name in ('linux', 'android', 'browser')}
expected = reports['linux']['checks']
assert len(expected) == 14 and len(set(expected)) == 14
for name, report in reports.items():
    assert report['checks'] == expected, (name, 'fixture mismatch')
    assert report['cycles'] == 20, name
    assert report['lifecycle'] == {key: True for key in (
        'forced_reload_cleanup', 'stale_response_rejected', 'stale_event_rejected',
        'colliding_request_ids', 'other_instance_survives')}, name
    assert report['fresh_context'] and report['engine_effect_survives'], name
    assert 0 < report['interruption_ms'] < 1000, name
assert reports['browser']['renderer_ticks'] > 0
assert reports['android']['ui_ticks'] > 0
assert reports['linux']['heap_limit'] and reports['android']['heap_limit']
for result in (reports['browser']['capped_memory'],
               json.loads((root / 'capped-memory-node.json').read_text())):
    assert result['fresh_module_works']
    assert len(result['probes']) == 4
    for probe in result['probes']:
        assert probe['rejected'] and probe['disposed'] and probe['failure'] is None
        assert probe['growth_rejected'] and probe['memory_bytes'] == 16 * 1024 * 1024
bounded = reports['browser']['disposable']
assert bounded['passed'] and bounded['cycles'] == 20
assert bounded['checks'] == expected
assert bounded['lifecycle'] == reports['linux']['lifecycle']
assert bounded['module_limit_bytes'] == 16 * 1024 * 1024
assert 0 < bounded['interruption_ms'] < 1000
for key in ('watchdog_cleanup', 'progress_during_hang',
            'full_suite_after_recovery', 'host_resources_empty'):
    assert bounded[key], key
assert {p['name'] for p in bounded['failures']} == {
    'single_buffer', 'repeated_buffers', 'repeated_strings', 'objects'}
for probe in bounded['failures']:
    assert probe['memory_bytes'] == 16 * 1024 * 1024
    for key in ('retired', 'resources_released', 'survivor_works', 'replacement_works'):
        assert probe[key], (probe['name'], key)
policy = bounded['policy']
assert policy['passed'] and policy['worker_budget'] and policy['exact_boundaries']
assert policy['resources_empty']
assert len(policy['cases']) == 23 and len(set(policy['cases'])) == 23
assert policy['limits'] == dict(wireBytes=32768, commandBytes=65536, depth=16,
    nodes=2048, inFlight=32, commands=64, subscriptions=16, timers=16, stalled=16, workers=8)
native = reports['linux']['policy']
assert reports['android']['policy'] == native
assert native['passed'] and native['parent_validation'] and native['exact_boundaries']
assert native['resources_empty'] and len(native['cases']) == 16
assert native['wire_bytes'] == 32768 and native['queued_requests'] == 32
assert native['subscriptions'] == native['stalled_calls'] == 16
process = json.loads((root / 'native-process.json').read_text())
assert process['host'] == 'linux' and process['resources_empty']
assert {p['fault'] for p in process['faults']} == {'abort', 'hang'}
for fault in process['faults']:
    for key in ('replacement_full_suite', 'replacement_works', 'resources_retired',
                'supervisor_survived', 'survivor_progress'):
        assert fault[key], (fault['fault'], key)
print('Native: 16 abuse cases pass on Linux and Android; Linux child abort/hang recovery passes.')
print('Browser bridge: 23 abuse cases plus exact resource boundaries pass.')
print('Capped disposable Workers pass the full suite, OOM isolation and watchdog recovery.')
print('Lifecycle cleanup and stale-generation checks pass on all 3 hosts.')
print('Capped WASM pressure probes pass in Node and Chromium.')
print('14 shared behavioral checks match across all 3 hosts (20 cycles each).')
print('Uncapped browser runtime qualified:', reports['browser']['qualified'])
print('Full P0 remains open; see docs/runtime-prototype.md.')
