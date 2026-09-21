#!/usr/bin/env python3
"""P4 qualification: immutable source evidence, emulator-only native tests, verified cleanup."""
import argparse,datetime,hashlib,json,os,pathlib,subprocess,sys,zipfile
p=argparse.ArgumentParser();p.add_argument('--emulator',required=True);p.add_argument('--target-dir',default='/tmp/talia-p4-target');args=p.parse_args()
if not args.emulator.startswith('emulator-'):raise SystemExit('Only an Android emulator is allowed')
root=pathlib.Path(__file__).resolve().parents[1];os.chdir(root);output=root/'engine/results/p4.json'
def hashes():
 paths=subprocess.check_output(['git','ls-files','-co','--exclude-standard','engine','dashboard'],text=True).splitlines()
 return {s:hashlib.sha256(pathlib.Path(s).read_bytes()).hexdigest() for s in sorted(set(paths)) if '/results/' not in s and pathlib.Path(s).is_file()}
def save():output.parent.mkdir(exist_ok=True);output.write_text(json.dumps(r,indent=2)+'\n')
source=hashes();binary=str(pathlib.Path(args.target_dir).resolve()/'debug/talia-engine');bindir=str(pathlib.Path(binary).parent)
env={**os.environ,'TALIA_BIN_DIR':bindir,'TALIA_ENGINE_BIN':binary,'TALIA_DELIVERY_FIXTURE':bindir+'/examples/delivery_fixture','TALIA_EMULATOR':args.emulator,'TALIA_ANDROID_ABI':'x86_64'}
r={'date':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':['web','android-emulator'],'base_commit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_sha256':source,'checks':{},'qualified':False,'missing_platforms':[],'required_checks':[]}
def run(name,cmd):
 r['required_checks'].append(name);print('Running: '+name,flush=True)
 if hashes()!=source:raise RuntimeError('Sources changed before '+name)
 try:
  done=subprocess.run(cmd,env=env,capture_output=True,text=True,timeout=600)
  result={'command':cmd,'exit_code':done.returncode,'stdout':done.stdout,'stderr':done.stderr}
 except subprocess.TimeoutExpired as e:result={'command':cmd,'exit_code':124,'stdout':str(e.stdout),'stderr':str(e.stderr)}
 r['checks'][name]=result;save()
 if result['exit_code']:raise RuntimeError(name+' failed: '+result['stderr'][-1800:]+result['stdout'][-1200:])
 result['results']=[]
 for line in result['stdout'].splitlines():
  try:v=json.loads(line)
  except ValueError:continue
  if isinstance(v,dict) and 'passed' in v:
   result['results'].append(v)
   if not v['passed']:raise RuntimeError(name+' reported failed checks')
 save()
def cleanup(name):
 cmd=['adb','-s',args.emulator,'shell','pm','list','packages','com.lelloman.talia.dashboard'];out=subprocess.check_output(cmd,text=True)
 forwards=subprocess.check_output(['adb','-s',args.emulator,'reverse','--list'],text=True)
 if 'package:com.lelloman.talia.dashboard' in out or forwards.strip()!=baseline_reverse.strip():raise RuntimeError(name+' leaked app or port mappings')
 r['checks'][name]['cleanup']={'app_absent':True,'reverse_mappings_restored':True};save()
baseline_reverse=''
try:
 if subprocess.check_output(['adb','-s',args.emulator,'shell','getprop','sys.boot_completed'],text=True).strip()!='1':raise RuntimeError('Emulator is not booted')
 if 'package:com.lelloman.talia.dashboard' in subprocess.check_output(['adb','-s',args.emulator,'shell','pm','list','packages','com.lelloman.talia.dashboard'],text=True):raise RuntimeError('Use a clean disposable emulator')
 baseline_reverse=subprocess.check_output(['adb','-s',args.emulator,'reverse','--list'],text=True)
 run('rust',['cargo','test','--manifest-path','engine/Cargo.toml','--locked','--offline','--target-dir',args.target_dir])
 run('native-unit',['cargo','test','--manifest-path','dashboard/native/Cargo.toml','--locked','--offline'])
 run('build-engine',['cargo','build','--manifest-path','engine/Cargo.toml','--locked','--offline','--target-dir',args.target_dir,'--bins','--examples'])
 run('shared-js',['node','--test','engine/tests/value.test.mjs','engine/tests/client.test.mjs','dashboard/tests/compiler.test.mjs','dashboard/tests/vm.test.mjs'])
 for name in ['transport','monitoring','clients']:run(name,['python3','engine/tests/'+name+'.py',binary])
 run('build-web',['bash','dashboard/build-web.sh'])
 run('build-android',['bash','dashboard/build-android.sh'])
 apk=pathlib.Path('dashboard/android/app/build/outputs/apk/debug/app-debug.apk');assets={}
 with zipfile.ZipFile(apk) as z:
  for entry,path in [('assets/ui.js','dashboard/shared/ui.js'),('assets/vm.js','dashboard/shared/vm.js'),('assets/live.js','dashboard/shared/live.js'),('assets/value.js','engine/shared/value.js')]:
   digest=hashlib.sha256(z.read(entry)).hexdigest();assert digest==hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest();assets[entry]=digest
  assets['lib/x86_64/libtalia_dashboard_runtime.so']=hashlib.sha256(z.read('lib/x86_64/libtalia_dashboard_runtime.so')).hexdigest()
 r['apk']={'abi':'x86_64','sha256':hashlib.sha256(apk.read_bytes()).hexdigest(),'assets':assets};save()
 for name,options in [('mcp_authoring',['--clients']),('mcp_engine',['--clients']),('mcp_live',['--android'])]:
  run(name,['python3','engine/tests/'+name+'.py',*options]);cleanup(name)
 for name in ['registration','delivery']:
  run(name+'-web',['node','dashboard/tests/'+name+'.mjs'])
  run(name+'-android',['python3','dashboard/tests/'+name+'_android.py',args.emulator]);cleanup(name+'-android')
 run('p0-provenance',['python3','spikes/qualification/check.py','spikes/qualification/results/full-2026-09-19.json'])
 if hashes()!=source:raise RuntimeError('Sources changed during qualification')
 r['qualified']=all(r['checks'][name]['exit_code']==0 for name in r['required_checks'])
except Exception as e:r['error']=str(e)
finally:save();print('Evidence: '+str(output),flush=True)
if not r['qualified']:print(r.get('error','Qualification incomplete'),file=sys.stderr);sys.exit(1)
print(json.dumps({'qualified':True,'checks':len(r['required_checks']),'platforms':r['scope']}))
