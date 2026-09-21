#!/usr/bin/env python3
"""Saved catalog delivery, explicit adoption and offline baseline on an emulator."""
import json,os,pathlib,subprocess,tempfile,time,sys,sqlite3,re,xml.etree.ElementTree as ET,shlex
serial=sys.argv[1] if len(sys.argv)>1 else 'emulator-5570'
if not serial.startswith('emulator-'):raise SystemExit('emulator required')
package='com.lelloman.talia.dashboard';binary=os.environ.get('TALIA_ENGINE_BIN','/tmp/talia-p3-target/debug/talia-engine');fixture=os.environ.get('TALIA_DELIVERY_FIXTURE','/tmp/talia-p3-target/debug/examples/delivery_fixture')
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check).stdout
def report():
 try:return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
 except Exception:return {}
def wait(f,timeout=35):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  if f():return report()
  time.sleep(.2)
 raise AssertionError(report())
def tap(label):
 adb('shell','uiautomator','dump','/sdcard/talia-delivery.xml');root=ET.fromstring(adb('shell','cat','/sdcard/talia-delivery.xml'))
 for node in root.iter('node'):
  if node.get('text')==label:
   x1,y1,x2,y2=map(int,re.findall(r'\d+',node.get('bounds')));adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2));return
 raise AssertionError('Missing '+label)
with tempfile.TemporaryDirectory(prefix='talia-delivery-native-') as temp:
 db=str(pathlib.Path(temp)/'engine.db');engine=None;engine_port=0;server=None
 def start_engine():
  global engine,engine_port
  engine=subprocess.Popen([binary,db,str(engine_port),'--seed'],stdout=subprocess.PIPE,text=True);engine_port=json.loads(engine.stdout.readline())['port']
 def stop_engine():engine.terminate();engine.wait(timeout=10)
 def edit(action):stop_engine();subprocess.run([fixture,db,action],check=True);start_engine()
 try:
  start_engine();edit('second');server=subprocess.Popen(['python3','dashboard/serve.py','0'],env={**os.environ,'TALIA_ENGINE_DB':db,'TALIA_ENGINE_PORT':str(engine_port)},stdout=subprocess.PIPE,text=True);port=json.loads(server.stdout.readline())['port']
  wait(lambda:adb('shell','getprop','sys.boot_completed').strip()=='1',timeout=90)
  adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{port}',f'tcp:{port}');adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',str(port),'--ez','durable','true','--es','dashboard_params',shlex.quote('{"sidebar":true}'),'--es','dashboard','monitor')
  wait(lambda:report().get('registration',{}).get('connected') and report().get('state',{}).get('history'));initial=report();assert initial['definitionRevision'].startswith('catalog-');assert initial['sidebar']
  tap('Controls');tap('Apply value');wait(lambda:any(a.get('status')=='complete' for a in report().get('actions',[])))
  adb('shell','am','start','-W','-n',package+'/.MainActivity','--es','live',shlex.quote('void 0'));wait(lambda:report().get('dirty'));edit('update');wait(lambda:report().get('updateAvailable'));assert report()['definitionRevision']==initial['definitionRevision'];assert report()['dirty']
  tap('Reload dashboard');wait(lambda:report().get('definitionRevision')!=initial['definitionRevision'] and not report().get('dirty') and 25 in report().get('state',{}).get('history',[]));loaded=report();assert loaded['subscriptions']==1 and loaded['sidebar'];assert loaded['registration']['liveInstanceId']!=initial['registration']['liveInstanceId']
  stop_engine();wait(lambda:report().get('connection')=='disconnected');tap('Reload dashboard');wait(lambda:report().get('cachedBaseline') and not report().get('dirty'));assert report()['definitionRevision']==loaded['definitionRevision'];assert report()['sidebar'];start_engine();tap('Reload dashboard');wait(lambda:not report().get('cachedBaseline') and 25 in report().get('state',{}).get('history',[]))
  before=report();edit('delete');tap('Reload dashboard');time.sleep(2);assert report()['registration']['liveInstanceId']==before['registration']['liveInstanceId'];assert not report().get('failure')
  adb('shell','am','force-stop',package);adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',str(port),'--ez','durable','true','--es','dashboard','secondary','--es','dashboard_params',shlex.quote('{"sidebar":false}'));wait(lambda:report().get('dashboardId')=='secondary' and report().get('state',{}).get('history'));assert not report()['sidebar'];second=report();adb('shell','am','force-stop',package);adb('shell','am','start','-W','-n',package+'/.MainActivity');wait(lambda:report().get('dashboardId')=='secondary' and report().get('registration',{}).get('liveInstanceId')!=second['registration']['liveInstanceId'] and report().get('state',{}).get('history'))
  print(json.dumps({'passed':True,'checks':['saved catalog package','assignment parameters','shared update without adoption','reload clears edits and replaces live instance','one current subscription','server effect retained','scoped cached offline baseline','online reload','deleted dashboard preserves running instance','saved selection survives process recreation']}))
 finally:
  adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False)
  if server:adb('reverse','--remove',f'tcp:{port}',check=False);server.terminate();server.wait(timeout=10)
  adb('shell','rm','-f','/sdcard/talia-delivery.xml',check=False)
  if engine and engine.poll() is None:stop_engine()
