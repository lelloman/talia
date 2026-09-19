#!/usr/bin/env python3
"""Run the transport APK through ADB, collect evidence, then always uninstall it."""
import argparse,hashlib,json,subprocess,time,zipfile
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('serial');p.add_argument('port',type=int);p.add_argument('--name',choices=['android','android-arm64'],required=True);p.add_argument('--runtime-regression',action='store_true');args=p.parse_args()
root=Path(__file__).resolve().parent;runtime=root.parent/'runtime';apk=runtime/'android/app/build/outputs/apk/debug/app-debug.apk';package='com.lelloman.talia.spike'
def adb(*cmd,check=True):
 r=subprocess.run(['adb','-s',args.serial,*cmd],capture_output=True,text=True,timeout=45)
 if check and r.returncode:raise RuntimeError(r.stderr+r.stdout)
 return r.stdout.strip()
def collect(activity,tag,extra=()):
 adb('shell','am','force-stop',package)
 adb('shell','am','start','-W','-n',package+'/'+activity,*extra)
 pid=adb('shell','pidof',package);assert pid
 for _ in range(90):
  lines=adb('logcat','-d','--pid='+pid,'-v','raw','-s',tag+':I','*:S').splitlines()
  if any(s.startswith('UI_TICKS=') for s in lines):break
  time.sleep(.5)
 else:raise RuntimeError('Activity report timed out')
 report=json.loads(next(s for s in lines if s.startswith('{')))
 report['ui_ticks']=int(next(s.split('=',1)[1] for s in lines if s.startswith('UI_TICKS=')))
 return report,lines
def save(path,data):path.write_text(json.dumps(data,indent=2)+'\n')
try:
 adb('reverse',f'tcp:{args.port}',f'tcp:{args.port}')
 print(adb('install','-r',str(apk)),flush=True)
 installed=adb('shell','sha256sum',adb('shell','pm','path',package).removeprefix('package:')).split()[0]
 assert installed==hashlib.sha256(apk.read_bytes()).hexdigest()
 report,_=collect('.TransportActivity','TaliaTransport',('--ei','transport_port',str(args.port)))
 assert report.get('done') and 'error' not in report and len(report['checks'])==25 and report['ui_ticks']>0,report
 report['android_sdk']=int(adb('shell','getprop','ro.build.version.sdk'));report['android_release']=adb('shell','getprop','ro.build.version.release')
 report['device_kind']='physical' if args.name.endswith('arm64') else 'emulator'
 report['model']=adb('shell','getprop','ro.product.model');save(root/'results'/(args.name+'.json'),report)
 if args.runtime_regression:
  regression,lines=collect('.MainActivity','TaliaRuntimeSpike')
  assert len(regression['checks'])==14 and len(regression['execution'])==27 and regression['ui_ticks']>0
  process=json.loads(next(s.split('=',1)[1] for s in lines if s.startswith('PROCESS_RESULT=')))
  assert process['passed']
  for name,data in [(args.name,regression),(args.name.replace('android','android-process'),process)]:
   path=runtime/'results'/(name+'.json');old=json.loads(path.read_text());old.update(data);save(path,old)
 with zipfile.ZipFile(apk) as z:
  libs=[n for n in z.namelist() if n.endswith('.so')];assert len(libs)==1
  library=hashlib.sha256(z.read(libs[0])).hexdigest()
 provenance={'apk_sha256':installed,'installed_apk_sha256':installed,'packaged_library':libs[0],'packaged_library_sha256':library}
 save(root/'results'/(args.name+'-apk.json'),provenance)
 if args.name=='android-arm64' and args.runtime_regression:
  path=runtime/'results/inputs-arm64.json';inputs=json.loads(path.read_text());inputs.update({k:provenance[k] for k in ['apk_sha256','installed_apk_sha256','packaged_library_sha256']})
  inputs['sha256']={n:hashlib.sha256((runtime/n).read_bytes()).hexdigest() for n in inputs['sha256']};save(path,inputs)
 print(f"PASS {args.name}: 25 transport checks; UI ticks {report['ui_ticks']}",flush=True)
finally:
 adb('shell','am','force-stop',package,check=False);print('Uninstall:',adb('uninstall',package,check=False),flush=True)
 adb('reverse','--remove',f'tcp:{args.port}',check=False)
 assert not adb('shell','pm','list','packages',package)
 assert not adb('shell','pidof',package,package+':runtime_a',package+':runtime_b',check=False)
 print('Verified package and test processes absent; reverse mapping removed.',flush=True)
