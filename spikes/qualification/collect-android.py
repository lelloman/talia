#!/usr/bin/env python3
"""Collect current APK evidence; always remove the test installation."""
import hashlib,json,subprocess,sys,time,zipfile
from pathlib import Path
serial,port=sys.argv[1:3];package='com.lelloman.talia.spike'
apk=Path('spikes/runtime/android/app/build/outputs/apk/debug/app-debug.apk')
def adb(*args,check=True):
 return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check,timeout=60).stdout.strip()
def collect(activity,tag,extra=()):
 adb('shell','am','force-stop',package);adb('shell','am','start','-W','-n',package+'/'+activity,*extra)
 pid=adb('shell','pidof',package);assert pid
 for _ in range(120):
  lines=adb('logcat','-d','--pid='+pid,'-v','raw','-s',tag+':I','*:S').splitlines()
  if any(s.startswith('UI_TICKS=') for s in lines):break
  time.sleep(.5)
 else:raise RuntimeError('Activity report timeout')
 report=json.loads(next(s for s in lines if s.startswith('{')))
 report['ui_ticks']=int(next(s.split('=',1)[1] for s in lines if s.startswith('UI_TICKS=')))
 assert report['ui_ticks']>0
 return report,lines
try:
 adb('reverse','tcp:'+port,'tcp:'+port);adb('install','-r',str(apk))
 installed=adb('shell','sha256sum',adb('shell','pm','path',package).removeprefix('package:')).split()[0]
 assert installed==hashlib.sha256(apk.read_bytes()).hexdigest()
 native,lines=collect('.MainActivity','TaliaRuntimeSpike')
 process=json.loads(next(s.split('=',1)[1] for s in lines if s.startswith('PROCESS_RESULT=')))
 transport,_=collect('.TransportActivity','TaliaTransport',('--ei','transport_port',port))
 with zipfile.ZipFile(apk) as archive:
  libs=[n for n in archive.namelist() if n.endswith('.so')];assert len(libs)==1
  library={'path':libs[0],'sha256':hashlib.sha256(archive.read(libs[0])).hexdigest()}
 report={'runtime':native,'process':process,'transport':transport,'apk_sha256':installed,'library':library,
  'sdk':adb('shell','getprop','ro.build.version.sdk'),'model':adb('shell','getprop','ro.product.model'),'abi':adb('shell','getprop','ro.product.cpu.abi')}
finally:
 adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False);adb('reverse','--remove','tcp:'+port,check=False)
 assert not adb('shell','pm','list','packages',package)
 assert not adb('shell','pidof',package,package+':runtime_a',package+':runtime_b',check=False)
report['cleanup_verified']=True
print(json.dumps(report))
