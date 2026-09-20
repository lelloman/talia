#!/usr/bin/env python3
"""Freeze sources and record real P2 checks. Missing physical execution never qualifies."""
import argparse,hashlib,json,pathlib,subprocess,os,datetime,sys
p=argparse.ArgumentParser();p.add_argument('--emulator');p.add_argument('--physical');p.add_argument('--target-dir',default='engine/target');args=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[1];os.chdir(root)
output=root/'engine/results/current.json';output.parent.mkdir(exist_ok=True)
def hashes():
 paths=subprocess.check_output(['git','ls-files','-co','--exclude-standard','engine','dashboard'],text=True).splitlines()
 return {s:hashlib.sha256(pathlib.Path(s).read_bytes()).hexdigest() for s in sorted(set(paths)) if '/results/' not in s and pathlib.Path(s).is_file()}
r={'date':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_sha256':hashes(),'checks':{},'apks':{},'qualified':False,'missing_platforms':[]}
def run(name,cmd,env=None,json_result=False):
 print('Running: '+name,flush=True);result=subprocess.run(cmd,capture_output=True,text=True,env={**os.environ,**(env or {})},timeout=600);r['checks'][name]={'command':cmd,'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr}
 if result.returncode:raise RuntimeError(name+' failed: '+result.stderr[-2000:]+result.stdout[-2000:])
 if json_result:r['checks'][name]['result']=json.loads(result.stdout.strip().splitlines()[-1])
 return result
binary=str(pathlib.Path(args.target_dir).resolve()/'debug/talia-engine')
try:
 run('rust',['cargo','test','--manifest-path','engine/Cargo.toml','--offline','--target-dir',args.target_dir])
 run('build-engine',['cargo','build','--manifest-path','engine/Cargo.toml','--offline','--target-dir',args.target_dir])
 for name in ['value','client']:run(name,['node',f'engine/tests/{name}.test.mjs'])
 for name in ['compiler','vm']:run(name,['node',f'dashboard/tests/{name}.test.mjs'])
 run('transport',['python3','engine/tests/transport.py',binary],json_result=True)
 run('build-web',['bash','dashboard/build-web.sh'])
 run('web',['node','engine/tests/web.mjs',binary],json_result=True)
 for name in ['web','responsive','updates','lifecycle']:run('p1-'+name,['node',f'dashboard/tests/{name}.mjs'],json_result=True)
 for name,abi,serial in [('android','x86_64',args.emulator),('android-arm64','arm64-v8a',args.physical)]:
  run('build-'+name,['bash','dashboard/build-android.sh'],{'TALIA_ANDROID_ABI':abi})
  apk=pathlib.Path('dashboard/android/app/build/outputs/apk/debug/app-debug.apk');r['apks'][name]={'abi':abi,'sha256':hashlib.sha256(apk.read_bytes()).hexdigest()}
  connected=subprocess.check_output(['adb','devices'],text=True)
  if not serial or serial+'\tdevice' not in connected:r['missing_platforms'].append(name);continue
  run(name,['python3','engine/tests/android.py',serial,binary],json_result=True)
  run('p1-'+name,['python3','dashboard/tests/android.py',serial],json_result=True)
 run('p0-provenance',['python3','spikes/qualification/check.py','spikes/qualification/results/full-2026-09-19.json'])
 if r['source_sha256']!=hashes():raise RuntimeError('sources changed during qualification')
 r['qualified']=not r['missing_platforms']
except Exception as e:r['error']=str(e)
finally:output.write_text(json.dumps(r,indent=2)+'\n');print('Evidence: '+str(output),flush=True)
if r.get('error'):print(r['error'],file=sys.stderr);sys.exit(1)
sys.exit(0 if r['qualified'] else 2)
