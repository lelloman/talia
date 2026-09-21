#!/usr/bin/env python3
"""Build and qualify P1; a missing physical run is explicitly incomplete."""
import argparse,subprocess,pathlib,os,json,hashlib,zipfile,sys
from datetime import datetime,timezone
ROOT=pathlib.Path(__file__).resolve().parents[1];os.chdir(ROOT)
p=argparse.ArgumentParser();p.add_argument('--emulator',required=True);p.add_argument('--physical');p.add_argument('--output',default='dashboard/results/current.json');p.add_argument('--p0-regression',action='store_true');args=p.parse_args()
def digest(path):return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
def inputs():
 files={}
 for folder,dirs,names in os.walk('dashboard'):
  dirs[:]=[d for d in dirs if d not in {'target','dist','build','.gradle','jniLibs','generated','results','__pycache__','node_modules'}]
  for name in names:
   path=pathlib.Path(folder)/name
   if name!='local.properties':files[str(path)]=digest(path)
 for path in ['spikes/runtime/package-lock.json','spikes/transport/server/src/main.rs','spikes/transport/server/Cargo.lock','docs/runtime-contract.md']:files[path]=digest(path)
 return files
report={'date':datetime.now(timezone.utc).isoformat(),'base_commit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_sha256':inputs(),'commands':[],'platforms':{},'qualified':False}
def run(command,env=None,parse=False,allowed=(0,)):
 print('Running: '+' '.join(command),flush=True)
 completed=subprocess.run(command,env=env,text=True,capture_output=True,timeout=900)
 report['commands'].append({'command':command,'exit_code':completed.returncode})
 if completed.returncode not in allowed:raise RuntimeError(completed.stdout+'\n'+completed.stderr)
 return json.loads(completed.stdout) if parse else completed.stdout
try:
 report['unit_tests']={}
 for file in ['compiler','vm']:report['unit_tests'][file]=run(['node','--test','--test-reporter=tap',f'dashboard/tests/{file}.test.mjs'])
 report['native_tests']=run(['cargo','test','--manifest-path','dashboard/native/Cargo.toml','--locked','--offline'])
 run(['cargo','build','--manifest-path','spikes/transport/server/Cargo.toml','--locked','--offline'])
 run(['bash','dashboard/build-web.sh'])
 report['package_revision']=json.loads(pathlib.Path('dashboard/generated/monitor.json').read_text())['revision']
 report['platforms']['browser']={name:run(['node',f'dashboard/tests/{name}.mjs'],parse=True) for name in ['web','responsive','updates','lifecycle']}
 report['apks']={}
 for name,serial,abi in [('android',args.emulator,'x86_64'),('android-arm64',args.physical,'arm64-v8a')]:
  report.setdefault('build_logs',{})[name]=run(['bash','dashboard/build-android.sh'],env={**os.environ,'TALIA_ANDROID_ABI':abi})[-12000:]
  apk='dashboard/android/app/build/outputs/apk/debug/app-debug.apk'
  with zipfile.ZipFile(apk) as archive:
   for source,asset in [('dashboard/generated/monitor.json','monitor.json'),('dashboard/shared/vm.js','vm.js'),('dashboard/shared/ui.js','ui.js')]:assert archive.read('assets/'+asset)==pathlib.Path(source).read_bytes(),('asset mismatch',asset)
  report['apks'][name]={'sha256':digest(apk),'abi':abi,'package_revision':report['package_revision'],'shared_assets_verified':True}
  if serial:
   result=run(['python3','dashboard/tests/android.py',serial],parse=True);assert result['passed'] and result['cleanup_verified'];report['platforms'][name]=result
   assert result['report']['state']==report['platforms']['browser']['lifecycle']['final']['state'],'observable cross-client state mismatch'
 report['p0_baseline']=run(['python3','spikes/qualification/check.py','spikes/qualification/results/full-2026-09-19.json'])
 if args.p0_regression:
  command=['python3','spikes/qualification/run.py','--android',args.emulator,'--output','dashboard/results/p0-regression.json']
  if args.physical:command+=['--physical',args.physical]
  run(command,allowed=(0,2))
 if pathlib.Path('dashboard/results/p0-regression.json').exists():
  report['p0_regression']=run(['python3','spikes/qualification/check.py','dashboard/results/p0-regression.json','--allow-missing-physical'])
  report['p0_regression_sha256']=digest('dashboard/results/p0-regression.json')
 assert report['source_sha256']==inputs(),'source changed during qualification'
 report['missing_platforms']=sorted({'browser','android','android-arm64'}-report['platforms'].keys())
 report['qualified']=not report['missing_platforms']
except Exception as e:
 report['error']=str(e);raise
finally:
 pathlib.Path(args.output).parent.mkdir(parents=True,exist_ok=True);pathlib.Path(args.output).write_text(json.dumps(report,indent=2)+'\n');print('Evidence: '+args.output,flush=True)
if not report['qualified']:sys.exit(2)
