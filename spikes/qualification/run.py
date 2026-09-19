#!/usr/bin/env python3
"""Run current P0 fixtures without rewriting any historical experiment reports."""
import argparse,hashlib,json,os,subprocess,sys,time
from pathlib import Path
from datetime import datetime,timezone
from importlib.machinery import SourceFileLoader
ROOT=Path(__file__).resolve().parents[2];os.chdir(ROOT)
p=argparse.ArgumentParser();p.add_argument('--android',required=True,help='x86_64 emulator serial');p.add_argument('--physical',help='ARM64 phone serial');p.add_argument('--output',type=Path,default=Path('spikes/qualification/results/current.json'));a=p.parse_args()
commands=[]
def run(command,env=None,parse=False):
 commands.append(command);print('Running: '+' '.join(command),flush=True)
 result=subprocess.run(command,capture_output=True,text=True,check=False,env=env,timeout=240)
 if result.returncode:raise RuntimeError(result.stdout+'\n'+result.stderr)
 return json.loads(result.stdout) if parse else result.stdout.strip()
def sources():
 found={}
 for folder,dirs,files in os.walk('spikes'):
  dirs[:]=[d for d in dirs if d not in {'target','node_modules','dist','build','.gradle','jniLibs','results','__pycache__'}]
  for name in files:
   file=Path(folder)/name
   if file.suffix in {'.rs','.js','.mjs','.py','.toml','.lock','.java','.xml','.json','.html','.sh'}:
    found[str(file)]=hashlib.sha256(file.read_bytes()).hexdigest()
 found['docs/runtime-contract.md']=hashlib.sha256(Path('docs/runtime-contract.md').read_bytes()).hexdigest()
 return found
inputs=sources();report={'date':datetime.now(timezone.utc).isoformat(),'base_commit':run(['git','rev-parse','HEAD']),'source_sha256':inputs,'commands':commands,'versions':{'rust':run(['rustc','--version']),'node':run(['node','--version'])},'platforms':{}}
run(['cargo','build','--manifest-path','spikes/runtime/native/Cargo.toml','--locked'])
run(['cargo','build','--manifest-path','spikes/transport/server/Cargo.toml','--locked'])
run(['npm','--prefix','spikes/runtime','run','build']);run(['bash','spikes/transport/build-web.sh'])
env={**os.environ,'NODE_PATH':'spikes/runtime/node_modules'}
run(['spikes/runtime/node_modules/.bin/esbuild','spikes/lifecycle/web.js','--bundle','--format=esm','--outfile=spikes/lifecycle/dist/web.js'],env=env)
run(['node','--test','spikes/runtime/bridge-policy.test.mjs'])
server=subprocess.Popen(['spikes/transport/server/target/debug/talia-transport-spike','0'],stdout=subprocess.PIPE,text=True)
try:
 port=str(json.loads(server.stdout.readline())['port']);native='spikes/runtime/native/target/debug/talia-runtime-spike'
 report['server']=run(['python3','spikes/transport/test-server.py',port],parse=True)
 report['platforms']['linux']={'runtime':run([native],parse=True),'process':run([native,'--process-check'],parse=True),'transport':run([native,'--transport',port],parse=True)}
 report['platforms']['browser']={'runtime':run(['node','spikes/runtime/run-browser.mjs'],parse=True),'transport':run(['node','spikes/transport/run-browser.mjs',port],parse=True),'lifecycle':run(['node','spikes/lifecycle/test-browser.mjs',port],parse=True)}
 gradle=os.environ.get('TALIA_GRADLE')
 if not gradle:
  choices=list((Path.home()/'.gradle/wrapper/dists/gradle-8.13-bin').glob('*/gradle-8.13/bin/gradle'));assert len(choices)==1;gradle=str(choices[0])
 for name,serial,abi in [('android',a.android,'x86_64')]+([('android-arm64',a.physical,'arm64-v8a')] if a.physical else []):
  build_env={**os.environ,'TALIA_GRADLE':gradle,'TALIA_ANDROID_ABI':abi}
  run(['bash','spikes/runtime/build-android.sh'],env=build_env)
  data=run(['python3','spikes/qualification/collect-android.py',serial,port],parse=True)
  data['lifecycle']=run(['python3','spikes/lifecycle/test-android.py',serial,port],parse=True)
  report['platforms'][name]=data
 assert inputs==sources(),'source changed during qualification'
 missing=SourceFileLoader('qualification_check',str(ROOT/'spikes/qualification/check.py')).load_module().validate(report)
 report['missing_platforms']=missing;report['qualified']=not missing
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(report,indent=2)+'\n')
 print('Evidence: '+str(a.output)+'; qualified='+str(report['qualified']),flush=True)
 if missing:raise SystemExit(2)
finally:
 server.terminate()
 try:server.wait(timeout=10)
 except subprocess.TimeoutExpired:server.kill();server.wait()
