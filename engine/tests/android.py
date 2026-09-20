#!/usr/bin/env python3
import json,subprocess,sys,time,pathlib,tempfile,os,shlex,re,xml.etree.ElementTree as ET
serial=sys.argv[1];binary=sys.argv[2] if len(sys.argv)>2 else 'engine/target/debug/talia-engine';package='com.lelloman.talia.dashboard'
engine=server=None;port=None;engine_port=0
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check).stdout
with tempfile.TemporaryDirectory(prefix='talia-p2-native-') as temp:
 def start_engine():
  global engine,engine_port
  engine=subprocess.Popen([binary,temp+'/engine.db',str(engine_port),'--seed'],stdout=subprocess.PIPE,text=True);engine_port=json.loads(engine.stdout.readline())['port']
 def stop_engine():
  if engine and engine.poll() is None:engine.kill();engine.wait()
 def report():
  try:return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
  except Exception:return {}
 def wait(fn,timeout=25,allow_failure=False):
  end=time.monotonic()+timeout
  while time.monotonic()<end:
   r=report()
   if r.get('failure') and not allow_failure:raise AssertionError(r)
   if fn(r):return r
   time.sleep(.2)
  raise AssertionError(report())
 def live(source):adb('shell','am','start','-W','-n',package+'/.MainActivity','--es','live',shlex.quote(source))
 def xml():
  adb('shell','uiautomator','dump','/sdcard/talia-p2.xml');return ET.fromstring(adb('shell','cat','/sdcard/talia-p2.xml'))
 def tap(label):
  for n in xml().iter('node'):
   if n.get('text')==label:
    x1,y1,x2,y2=map(int,re.findall(r'\d+',n.get('bounds')));adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2));return
  raise AssertionError('missing control '+label)
 def text(label,timeout=20):
  end=time.monotonic()+timeout
  while time.monotonic()<end:
   if any(label in n.get('text','') or label in n.get('content-desc','') for n in xml().iter('node')):return
  raise AssertionError('missing text '+label)
 try:
  start_engine();server=subprocess.Popen(['python3','dashboard/serve.py','0'],env={**os.environ,'TALIA_ENGINE_DB':temp+'/engine.db','TALIA_ENGINE_PORT':str(engine_port)},stdout=subprocess.PIPE,text=True);port=json.loads(server.stdout.readline())['port']
  adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{port}',f'tcp:{port}');adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',str(port),'--ez','durable','true')
  wait(lambda r:len(r.get('state',{}).get('history',[]))>0)
  live("TaliaVM.replaceAction('special',c=>{const s=c.state();c.commit(s,{...s.value,value:Infinity,title:{text:undefined},screen:'controlsScreen'})});TaliaVM.dispatch('special',{target:'test',value:null})")
  wait(lambda r:r.get('dirty') and r.get('state',{}).get('screen')=='controlsScreen');text('Server value: ∞ (unavailable)')
  live("TaliaVM.dispatch('apply',{target:'test',value:null})")
  wait(lambda r:any(a.get('status')=='complete' for a in r.get('actions',[])))
  stop_engine();text('disconnected');start_engine();text('back online')
  r=wait(lambda r:r.get('connection') in ('back online',''));assert r['dirty'] and r['state']['screen']=='controlsScreen',r
  live("TaliaVM.dispatch('overview',{target:'test',value:null})");text('undefined');text('∞')
  assert all('WebView' not in n.get('class','') for n in xml().iter('node'))
  adb('shell','input','keyevent','KEYCODE_HOME');time.sleep(.5)
  adb('shell','am','start','-W','-n',package+'/.MainActivity');wait(lambda r:not r.get('paused') and r.get('dirty'))
  live("throw Error('qualification failure')");text('Dashboard stopped')
  stop_engine();text('disconnected');start_engine();text('back online');tap('Restart dashboard')
  wait(lambda r:not r.get('failure') and not r.get('dirty',True) and r.get('state',{}).get('history'),allow_failure=True)
  result={'passed':True,'serial':serial,'checks':['native widgets','exceptional values through native QuickJS and SQLite','unavailable slider','visible undefined/chart annotation','restart retains dirty VM and selected screen','connection states','pause/resume','connection survives dashboard failure','manual restart restores baseline'],'state':report()}
 finally:
  adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False)
  if port:adb('reverse','--remove',f'tcp:{port}',check=False)
  adb('shell','rm','-f','/sdcard/talia-p2.xml',check=False)
  if server and server.poll() is None:server.terminate();server.wait(timeout=5)
  stop_engine()
 assert not adb('shell','pm','path',package,check=False).strip()
 result['cleanup_verified']=True;print(json.dumps(result))
