#!/usr/bin/env python3
"""Require current source hashes and matching conformance on every required host."""
import argparse,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
def validate(report):
 for source,digest in report['source_sha256'].items():
  assert hashlib.sha256((ROOT/source).read_bytes()).hexdigest()==digest,('changed input',source)
 platforms=report['platforms'];expected=platforms['linux']['runtime']
 assert len(expected['checks'])==14 and len(expected['execution'])==27
 assert len(expected['capabilities']['cases'])==16
 assert len(expected['protected_execution']['checks'])>=20
 for name,platform in platforms.items():
  runtime=platform['runtime'];bounded=runtime['disposable'] if name=='browser' else runtime
  assert runtime['checks']==expected['checks'] and runtime['execution']==expected['execution'],name
  assert runtime['cycles']==20 and runtime['fresh_context'] and runtime['engine_effect_survives'],name
  for section in ('capabilities','protected_execution'):
   assert bounded[section]==expected[section],(name,section)
  assert all(runtime['lifecycle'].values()),name
  if name=='browser':
   assert bounded['passed'] and bounded['host_resources_empty'] and bounded['progress_during_hang']
   assert runtime['renderer_ticks']>0
  else:
   assert runtime['heap_limit'] and runtime['policy']['passed']
   process=platform['process'];assert process.get('passed',True) and process['resources_empty']
   for fault in process['faults']:assert fault['replacement_full_suite'] and fault['survivor_progress'] and fault['resources_retired']
  transport=platform['transport'];assert transport['done'] and 'error' not in transport
  assert len(transport['checks'])==25 and transport['checks']==platforms['linux']['transport']['checks']
  assert transport['http_requests_remaining']==0
  if name.startswith('android'):
   assert runtime['ui_ticks']>0 and transport['ui_ticks']>0 and platform['cleanup_verified']
   assert ('arm64-v8a' if name=='android-arm64' else 'x86_64')==platform['abi']
  if name!='linux':
   lifecycle=platform['lifecycle'];assert lifecycle['passed']
   final=lifecycle['final'];assert final['active'] and not final['failure'] and not final['local']['dirty']
   assert final['snapshot']['value']==62 and final['subscriptions']==1
   assert len(final['signals'])==3
   assert any('external failure' in c for c in lifecycle['checks'])
   assert sum('restart preserves' in c for c in lifecycle['checks'])==3
 assert len(report['server']['checks'])==10
 return sorted({'linux','browser','android','android-arm64'}-platforms.keys())
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('report',type=Path);p.add_argument('--allow-missing-physical',action='store_true');args=p.parse_args()
 missing=validate(json.loads(args.report.read_text()))
 print('Recorded platform checks and source provenance pass. Missing: '+(', '.join(missing) or 'none'))
 if missing and not (args.allow_missing_physical and missing==['android-arm64']):raise SystemExit(2)
