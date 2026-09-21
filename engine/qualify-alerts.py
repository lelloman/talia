#!/usr/bin/env python3
"""Reproducible alert qualification. Emulator only; all providers are local fixtures."""
import argparse,datetime,hashlib,json,os,pathlib,subprocess,sys,zipfile
p=argparse.ArgumentParser();p.add_argument('--emulator',required=True);p.add_argument('--target-dir',default='/tmp/talia-alert-target');a=p.parse_args()
if not a.emulator.startswith('emulator-'):raise SystemExit('Only an Android emulator is allowed')
root=pathlib.Path(__file__).resolve().parents[1];os.chdir(root);output=root/'engine/results/alerts.json';target=pathlib.Path(a.target_dir).resolve();env={**os.environ,'TALIA_BIN_DIR':str(target/'debug'),'TALIA_ENGINE_BIN':str(target/'debug/talia-engine'),'TALIA_EMULATOR':a.emulator,'TALIA_ANDROID_ABI':'x86_64'}
def hashes():
 paths=subprocess.check_output(['git','ls-files','-co','--exclude-standard','engine','dashboard'],text=True).splitlines();return {s:hashlib.sha256(pathlib.Path(s).read_bytes()).hexdigest() for s in sorted(set(paths)) if '/results/' not in s and pathlib.Path(s).is_file()}
source=hashes();r={'date':datetime.datetime.now(datetime.timezone.utc).isoformat(),'base_commit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'scope':['web','android-emulator','local-provider-fixtures'],'real_provider_delivery_verified':False,'source_sha256':source,'checks':{},'qualified':False}
def save():output.parent.mkdir(exist_ok=True);output.write_text(json.dumps(r,indent=2)+'\n')
def run(name,command):
 print('Running '+name,flush=True)
 if hashes()!=source:raise RuntimeError('Source changed before '+name)
 try:
  v=subprocess.run(command,env=env,capture_output=True,text=True,timeout=600);result={'command':command,'exit_code':v.returncode,'stdout':v.stdout,'stderr':v.stderr}
 except subprocess.TimeoutExpired as e:result={'command':command,'exit_code':124,'stdout':str(e.stdout),'stderr':str(e.stderr)}
 r['checks'][name]=result;save()
 if result['exit_code']:raise RuntimeError(name+' failed: '+result['stderr'][-2500:]+result['stdout'][-1500:])
 for line in result['stdout'].splitlines():
  try:value=json.loads(line)
  except ValueError:continue
  if isinstance(value,dict) and value.get('passed') is False:raise RuntimeError(name+' reported failure')
def cleanup(name):
 assert 'package:com.lelloman.talia.dashboard' not in subprocess.check_output(['adb','-s',a.emulator,'shell','pm','list','packages','com.lelloman.talia.dashboard'],text=True)
 assert subprocess.check_output(['adb','-s',a.emulator,'reverse','--list'],text=True).strip()==reverse.strip()
 r['checks'][name]['cleanup']={'app_absent':True,'reverse_mappings_restored':True};save()
try:
 assert subprocess.check_output(['adb','-s',a.emulator,'shell','getprop','sys.boot_completed'],text=True).strip()=='1'
 assert 'package:com.lelloman.talia.dashboard' not in subprocess.check_output(['adb','-s',a.emulator,'shell','pm','list','packages','com.lelloman.talia.dashboard'],text=True)
 reverse=subprocess.check_output(['adb','-s',a.emulator,'reverse','--list'],text=True)
 run('engine-tests',['cargo','test','--manifest-path','engine/Cargo.toml','--locked','--offline','--target-dir',str(target),'--','--test-threads=1'])
 run('native-runtime-tests',['cargo','test','--manifest-path','dashboard/native/Cargo.toml','--locked','--offline'])
 run('engine-build',['cargo','build','--manifest-path','engine/Cargo.toml','--locked','--offline','--target-dir',str(target),'--bins','--examples'])
 run('shared-js',['node','--test','engine/tests/value.test.mjs','engine/tests/client.test.mjs','dashboard/tests/compiler.test.mjs','dashboard/tests/vm.test.mjs'])
 run('web-build',['bash','dashboard/build-web.sh'])
 run('android-build',['bash','dashboard/build-android.sh'])
 apk=pathlib.Path('dashboard/android/app/build/outputs/apk/debug/app-debug.apk');assets={}
 with zipfile.ZipFile(apk) as z:
  for entry,path in [('assets/vm.js','dashboard/shared/vm.js'),('assets/ui.js','dashboard/shared/ui.js'),('assets/value.js','engine/shared/value.js')]:
   digest=hashlib.sha256(z.read(entry)).hexdigest();assert digest==hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest();assets[entry]=digest
 r['apk']={'sha256':hashlib.sha256(apk.read_bytes()).hexdigest(),'abi':'x86_64','assets':assets};save()
 run('monitoring-regression',['python3','engine/tests/monitoring.py',str(target/'debug/talia-engine')])
 run('mcp-authoring-regression',['python3','engine/tests/mcp_authoring.py'])
 run('alert-workflow',['python3','engine/tests/alert_workflow.py'])
 run('alert-controls',['python3','engine/tests/alert_controls.py']);cleanup('alert-controls')
 run('android-push',['python3','engine/tests/alert_push.py']);cleanup('android-push')
 gradle=os.environ.get('TALIA_GRADLE') or str(next(pathlib.Path.home().glob('.gradle/wrapper/dists/gradle-8.13-bin/*/gradle-8.13/bin/gradle')))
 run('release-manifest',[gradle,'-p','dashboard/android','--offline','--no-daemon','processReleaseMainManifest'])
 manifests=list(pathlib.Path('dashboard/android/app/build/intermediates/merged_manifests/release').rglob('AndroidManifest.xml')) or list(pathlib.Path('dashboard/android/app/build/intermediates/merged_manifest/release').rglob('AndroidManifest.xml'))
 assert manifests and all('AlertFixtureActivity' not in m.read_text() for m in manifests)
 r['checks']['release-manifest']['fixture_absent']=True
 assert hashes()==source;r['qualified']=True
except Exception as e:r['error']=str(e)
finally:save();print('Evidence: '+str(output),flush=True)
if not r['qualified']:print(r.get('error','Incomplete'),file=sys.stderr);sys.exit(1)
print(json.dumps({'qualified':True,'checks':len(r['checks']),'providers':'local fixtures only'}))
