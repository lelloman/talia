#!/usr/bin/env python3
"""Validate fresh capability reports without relabelling historical P0 evidence."""
import hashlib
import json
import sys
from pathlib import Path
root = Path(__file__).resolve().parent
folder = Path(sys.argv[1])
inputs = json.loads((folder / 'inputs.json').read_text())
for path, digest in inputs.items():
    assert hashlib.sha256((root / path).read_bytes()).hexdigest() == digest, path
expected = [f"{phase}: {case['name']}" for phase in ('saved', 'live')
            for case in json.loads((root / 'shared/capabilities.json').read_text())]
for platform in ('linux', 'browser', 'android'):
    report = json.loads((folder / f'{platform}.json').read_text())
    if platform == 'browser':
        assert report['renderer_ticks'] > 0
        report = report['disposable']
    if platform == 'android':
        assert report['ui_ticks'] > 0
    capability = report['capabilities']
    assert capability['cases'] == expected
    assert all(capability[key] for key in ('passed', 'dispatch_recheck', 'granted_operations', 'view_unchanged', 'survivor_works'))
print('Capability boundary checks passed on Linux, Chromium and native Android.')
