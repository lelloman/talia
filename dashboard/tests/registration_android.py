#!/usr/bin/env python3
"""Registration/lifecycle integration on a disposable emulator only."""
import json,os,pathlib,subprocess,tempfile,time,sys,sqlite3,re,xml.etree.ElementTree as ET,shlex
serial=sys.argv[1] if len(sys.argv)>1 else 'emulator-5570'
if not serial.startswith('emulator-'):raise SystemExit('emulator required')
package='com.lelloman.talia.dashboard'
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check).stdout
def wait(f,timeout=30):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  try:
   value=f()
   if value:return value
  except (OSError,ValueError,KeyError,subprocess.CalledProcessError):pass
  time.sleep(.2)
 raise AssertionError('Timed out: '+str(report()))
def report():return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
def tap(label):
 adb('shell','uiautomator','dump','/sdcard/talia-registry.xml');root=ET.fromstring(adb('shell','cat','/sdcard/talia-registry.xml'))
 for node in root.iter('node'):
  if node.get('text')==label:
   x1,y1,x2,y2=map(int,re.findall(r'\d+',node.get('bounds')));adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2));return
 raise AssertionError('Missing '+label)
with tempfile.TemporaryDirectory(prefix='talia-p4-native-') as temp:
 db=str(pathlib.Path(temp)/'engine.db');env={**os.environ,'TALIA_ENGINE_DB':db,'TALIA_ENGINE_BIN':os.environ.get('TALIA_ENGINE_BIN','/tmp/talia-p3-target/debug/talia-engine')}
 definition=pathlib.Path(temp)/'package.json';pkg=json.loads(pathlib.Path('dashboard/generated/monitor.json').read_text());definition.write_text(json.dumps(pkg))
 server=subprocess.Popen(['python3','dashboard/serve.py','0',str(definition)],env=env,stdout=subprocess.PIPE,text=True);port=json.loads(server.stdout.readline())['port']
 def slot():
  with sqlite3.connect(db) as conn:
   row=conn.execute('SELECT body FROM dashboard_slots').fetchone()
   return json.loads(row[0]) if row else None
 def start(*args):adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',str(port),*args)
 try:
  wait(lambda:adb('shell','getprop','sys.boot_completed').strip()=='1',timeout=90)
  adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{port}',f'tcp:{port}');start('--ez','durable','true');wait(lambda:report().get('registration',{}).get('connected'))
  initial=slot();assert initial['report']['lifecycle']=='active',initial
  adb('shell','am','start','-W','-n',package+'/.MainActivity','--es','live',shlex.quote('void 0'))
  wait(lambda:slot()['report']['dirty'] and slot()['report']['editRevision']==1)
  adb('shell','input','keyevent','KEYCODE_HOME');wait(lambda:slot()['report']['lifecycle']=='paused' and not slot()['report']['foreground'])
  assert slot()['liveInstanceId']==initial['liveInstanceId'];start();wait(lambda:slot()['report']['lifecycle']=='active');assert slot()['report']['dirty']
  tap('Reload dashboard');wait(lambda:slot()['liveInstanceId']!=initial['liveInstanceId'] and slot()['report']['lifecycle']=='active');clean=slot();assert not clean['report']['dirty'] and clean['report']['editRevision']==0
  adb('shell','am','start','-W','-n',package+'/.MainActivity','--es','live',shlex.quote("throw Error('registration failure fixture')"));wait(lambda:slot()['report']['lifecycle']=='failed');assert slot()['report']['foreground']
  adb('shell','input','keyevent','KEYCODE_HOME');wait(lambda:not slot()['report']['foreground']);start();wait(lambda:slot()['report']['foreground']);tap('Restart dashboard');wait(lambda:slot()['liveInstanceId']!=clean['liveInstanceId'] and slot()['report']['lifecycle']=='active')
  before=slot();adb('shell','am','force-stop',package);start();wait(lambda:slot()['liveInstanceId']!=before['liveInstanceId'] and slot()['report']['lifecycle']=='active');after=slot();assert after['clientId']==before['clientId'] and after['slotId']==before['slotId']
  # Persisted selection survives process recreation, separately from live instance identity.
  pkg['id']='secondary';definition.write_text(json.dumps(pkg));adb('shell','am','force-stop',package);start('--es','dashboard','secondary');wait(lambda:slot()['report']['dashboardId']=='secondary' and slot()['report']['lifecycle']=='active');adb('shell','am','force-stop',package);last=slot()['liveInstanceId'];start();wait(lambda:slot()['liveInstanceId']!=last and slot()['report']['dashboardId']=='secondary' and slot()['report']['lifecycle']=='active')
  public=json.dumps(report());assert 'credential' not in public and 'owner' not in public
  print(json.dumps({'passed':True,'checks':['persistent client and slot','dirty/edit reporting','background pause','foreground resume retains live ID','reload creates clean new instance','failure remains visible in registry','failed host background/foreground','process recreation replaces instance','selection survives process recreation','credentials absent from guest report']}))
 finally:
  adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False);adb('reverse','--remove',f'tcp:{port}',check=False);adb('shell','rm','-f','/sdcard/talia-registry.xml',check=False);server.terminate();server.wait(timeout=10)
