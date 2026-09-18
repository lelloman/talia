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
print('Capped disposable Workers pass the full suite, OOM isolation and watchdog recovery.')
print('Lifecycle cleanup and stale-generation checks pass on all 3 hosts.')
print('Capped WASM pressure probes pass in Node and Chromium.')
print('14 shared behavioral checks match across all 3 hosts (20 cycles each).')
print('Uncapped browser runtime qualified:', reports['browser']['qualified'])
print('Full P0 remains open; see docs/runtime-prototype.md.')
