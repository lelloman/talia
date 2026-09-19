#!/usr/bin/env python3
"""Validate saved transport evidence; does not run clients or claim production qualification."""
import hashlib,json
from pathlib import Path
root=Path(__file__).resolve().parent
reports={name:json.loads((root/'results'/f'{name}.json').read_text()) for name in ['linux','browser','android','android-arm64']}
expected=reports['linux']['checks']
assert len(expected)==25 and len(set(expected))==25
for name,report in reports.items():
    assert report['done'] and 'error' not in report,name
    assert report['checks']==expected,name
    assert report['http_requests_remaining']==0 and 1<report['peak_http_requests']<=32,name
assert reports['browser']['renderer_ticks']>0 and reports['browser']['memory_bytes']==16*1024*1024
for name,arch in [('android','x86_64'),('android-arm64','aarch64')]:
    assert reports[name]['arch']==arch and reports[name]['ui_ticks']>0
    apk=json.loads((root/'results'/f'{name}-apk.json').read_text())
    assert apk['apk_sha256']==apk['installed_apk_sha256'] and len(apk['apk_sha256'])==64
    abi='arm64-v8a' if arch=='aarch64' else 'x86_64'
    assert apk['packaged_library']==f'lib/{abi}/libtalia_runtime_spike.so'
    assert len(apk['packaged_library_sha256'])==64
server=json.loads((root/'results/server.json').read_text())
assert server['passed'] and len(server['checks'])==10
inputs=json.loads((root/'results/inputs.json').read_text())
for path,digest in inputs['sha256'].items():
    assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
print('25 transport checks match on Linux, Chromium, Android emulator and physical ARM64.')
print('10 additional server validation/concurrency/failure checks pass; source hashes match.')
print('Client requests drained; browser/Android UI heartbeats progressed.')
print('Production transport, durable outcomes and full P0 qualification remain open.')
