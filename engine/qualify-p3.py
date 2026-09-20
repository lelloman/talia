#!/usr/bin/env python3
"""Reproducible P3 qualification. Resume only checks run against identical source bytes."""
import argparse,datetime,hashlib,json,os,pathlib,subprocess,sys,zipfile
p=argparse.ArgumentParser();p.add_argument('--emulator');p.add_argument('--physical');p.add_argument('--target-dir',default='engine/target');p.add_argument('--resume',action='store_true');p.add_argument('--rerun',nargs='*',default=[]);args=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[1];os.chdir(root);output=root/'engine/results/p3.json';output.parent.mkdir(exist_ok=True)
def hashes():
 paths=subprocess.check_output(['git','ls-files','-co','--exclude-standard','engine','dashboard'],text=True).splitlines()
 return {s:hashlib.sha256(pathlib.Path(s).read_bytes()).hexdigest() for s in sorted(set(paths)) if '/results/' not in s and pathlib.Path(s).is_file()}
def save():output.write_text(json.dumps(r,indent=2)+'\n')
source=hashes();r={'date':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_sha256':source,'checks':{},'apks':{},'qualified':False,'missing_platforms':[],'physical_device_requested':bool(args.physical)}
if args.resume and output.exists():
 old=json.loads(output.read_text())
 if old['source_sha256']!=source:raise SystemExit('Sources changed: start a new qualification run.')
 if old.get('physical_device_requested',True)!=bool(args.physical):raise SystemExit('Platform scope changed: start a new qualification run.')
 r=old;r['qualified']=False;r.pop('error',None);r['missing_platforms']=[]
required=[]
def run(name,cmd,env=None,json_result=False):
 required.append(name)
 if args.resume and name not in args.rerun and r['checks'].get(name,{}).get('exit_code')==0:print('Verified earlier: '+name,flush=True);return
 print('Running: '+name,flush=True)
 before=hashes()
 if before!=source:raise RuntimeError('sources changed before '+name)
 try:
  result=subprocess.run(cmd,capture_output=True,text=True,env={**os.environ,**(env or {})},timeout=600)
  check={'command':cmd,'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr}
 except subprocess.TimeoutExpired as e:
  check={'command':cmd,'exit_code':124,'stdout':str(e.stdout),'stderr':str(e.stderr)}
 previous=r['checks'].get(name)
 if previous:check['previous_attempts']=previous.get('previous_attempts',[])+[{k:v for k,v in previous.items() if k!='previous_attempts'}]
 r['checks'][name]=check;save()
 if check['exit_code']:raise RuntimeError(name+' failed: '+check['stderr'][-2400:]+check['stdout'][-1200:])
 if json_result:check['result']=json.loads(check['stdout'].strip().splitlines()[-1])
 save()
def apk(name,abi):
 path=pathlib.Path('dashboard/android/app/build/outputs/apk/debug/app-debug.apk')
 # A resumed already-qualified platform retains its original artifact record.
 if args.resume and name in r['apks'] and 'build-'+name not in args.rerun and all(r['checks'].get(k,{}).get('exit_code')==0 for k in [name,'p2-'+name,'p1-'+name]):return
 with zipfile.ZipFile(path) as z:
  checks={}
  for entry,local in [('assets/ui.js','dashboard/shared/ui.js'),('assets/vm.js','dashboard/shared/vm.js'),('assets/value.js','engine/shared/value.js'),('assets/monitoring.json','dashboard/generated/monitoring.json')]:
   actual=hashlib.sha256(z.read(entry)).hexdigest();expected=hashlib.sha256(pathlib.Path(local).read_bytes()).hexdigest();assert actual==expected,(entry,actual,expected);checks[entry]=actual
  native='lib/'+abi+'/libtalia_dashboard_runtime.so';checks[native]=hashlib.sha256(z.read(native)).hexdigest()
 r['apks'][name]={'abi':abi,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'assets':checks};save()
binary=str(pathlib.Path(args.target_dir).resolve()/'debug/talia-engine')
try:
 run('rust',['cargo','test','--manifest-path','engine/Cargo.toml','--offline','--target-dir',args.target_dir])
 run('native-unit',['cargo','test','--manifest-path','dashboard/native/Cargo.toml','--offline','--target-dir','/tmp/talia-p3-native-tests'])
 run('build-engine',['cargo','build','--manifest-path','engine/Cargo.toml','--offline','--target-dir',args.target_dir])
 for name in ['value','client']:run(name,['node',f'engine/tests/{name}.test.mjs'])
 for name in ['compiler','vm']:run(name,['node',f'dashboard/tests/{name}.test.mjs'])
 run('transport',['python3','engine/tests/transport.py',binary],json_result=True)
 run('monitoring-api',['python3','engine/tests/monitoring.py',binary],json_result=True)
 run('build-web',['bash','dashboard/build-web.sh'])
 run('web',['node','engine/tests/monitoring-web.mjs',binary],json_result=True)
 run('p2-web',['node','engine/tests/web.mjs',binary],json_result=True)
 for name in ['web','responsive','updates','lifecycle']:run('p1-'+name,['node',f'dashboard/tests/{name}.mjs'],json_result=True)
 platforms=[('android','x86_64',args.emulator)]
 if args.physical:platforms.append(('android-arm64','arm64-v8a',args.physical))
 for name,abi,serial in platforms:
  run('build-'+name,['bash','dashboard/build-android.sh'],{'TALIA_ANDROID_ABI':abi});apk(name,abi)
  connected=subprocess.check_output(['adb','devices'],text=True)
  if not serial or serial+'\tdevice' not in connected:r['missing_platforms'].append(name);continue
  run(name,['python3','engine/tests/monitoring-android.py',serial,binary],json_result=True)
  run('p2-'+name,['python3','engine/tests/android.py',serial,binary],json_result=True)
  run('p1-'+name,['python3','dashboard/tests/android.py',serial],json_result=True)
 run('p0-provenance',['python3','spikes/qualification/check.py','spikes/qualification/results/full-2026-09-19.json'])
 if source!=hashes():raise RuntimeError('sources changed during qualification')
 r['required_checks']=required;r['qualified']=not r['missing_platforms'] and all(r['checks'].get(n,{}).get('exit_code')==0 for n in required)
except Exception as e:r['error']=str(e)
finally:save();print('Evidence: '+str(output),flush=True)
if r.get('error'):print(r['error'],file=sys.stderr);sys.exit(1)
sys.exit(0 if r['qualified'] else 2)
