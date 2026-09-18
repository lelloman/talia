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
    assert report['fresh_context'] and report['engine_effect_survives'], name
    assert 0 < report['interruption_ms'] < 1000, name
assert reports['browser']['renderer_ticks'] > 0
assert reports['android']['ui_ticks'] > 0
assert reports['linux']['heap_limit'] and reports['android']['heap_limit']
print('14 shared behavioral checks match across all 3 hosts (20 cycles each).')
print('Browser qualified:', reports['browser']['qualified'])
print('Full P0 remains open; see docs/runtime-prototype.md.')
