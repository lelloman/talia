#!/usr/bin/env python3
"""Native monitoring dashboard qualification; owns and removes only the Talìa test app."""
import json,subprocess,sys,time,shlex,re,xml.etree.ElementTree as ET
from monitoring_fixture import Harness
serial=sys.argv[1];binary=sys.argv[2] if len(sys.argv)>2 else '/tmp/talia-p3-target/debug/talia-engine';package='com.lelloman.talia.dashboard';h=None

def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check,timeout=40).stdout
def report():
 try:return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
 except Exception:return {}
def wait(fn,timeout=30):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  r=report()
  if r.get('failure'):raise AssertionError(r)
  if fn(r):return r
  time.sleep(.2)
 raise AssertionError(report())
def xml():
 error=None
 for attempt in range(3):
  try:
   adb('shell','rm','-f','/sdcard/talia-p3.xml')
   dump=adb('shell','uiautomator','dump','/sdcard/talia-p3.xml')
   return ET.fromstring(adb('shell','cat','/sdcard/talia-p3.xml'))
  except (subprocess.CalledProcessError,subprocess.TimeoutExpired,ET.ParseError) as e:error=e;time.sleep(.3)
 raise AssertionError('UI hierarchy unavailable after three attempts') from error
def tap(label):
 for n in xml().iter('node'):
  if n.get('text')==label:
   x1,y1,x2,y2=map(int,re.findall(r'\d+',n.get('bounds')));adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2));return
 raise AssertionError('missing control '+label)
def live(source):adb('shell','am','start','-W','-n',package+'/.MainActivity','--es','live',shlex.quote(source))
def text(label,timeout=20):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  if any(label in n.get('text','') or label in n.get('content-desc','') for n in xml().iter('node')):return
 raise AssertionError('missing text '+label)
try:
 h=Harness(binary);adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{h.port}',f'tcp:{h.port}');adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',str(h.port),'--ez','durable','true','--es','dashboard','monitoring')
 wait(lambda r:r.get('state',{}).get('samples',{}).get('cpu',{}).get('value')==42 and r.get('subscriptions')==9 and len(r.get('state',{}).get('samples',{}))==9);text('CPU: 42%')
 assert all('WebView' not in n.get('class','') for n in xml().iter('node'))
 h.fixture.disk=5;wait(lambda r:r.get('state',{}).get('watchX')=='Disk X: investigation triggered');assert h.fixture.probes==1
 tap('Investigation');text('db: 60%')
 first=wait(lambda r:r.get('state',{}).get('runStatus')=='complete')['state']['samples']['monitor.investigate']['value']['run']['id']
 tap('Run investigation');wait(lambda r:r.get('state',{}).get('message')=='Investigation requested')
 wait(lambda r:r.get('state',{}).get('runStatus')=='complete' and r['state']['samples']['monitor.investigate']['value']['run']['id']!=first);assert h.fixture.probes==2
 live("TaliaVM.replaceAction('mark',c=>{const s=c.state();c.commit(s,{...s.value,note:'temporary'})});TaliaVM.dispatch('mark',{target:'test',value:null})")
 wait(lambda r:r.get('dirty') and r.get('state',{}).get('note')=='temporary')
 before=h.fixture.calls;adb('shell','input','keyevent','KEYCODE_HOME');wait(lambda r:r.get('paused') and r.get('subscriptions')==0);time.sleep(.4);assert h.fixture.calls>before
 adb('shell','am','start','-W','-n',package+'/.MainActivity');wait(lambda r:not r.get('paused') and r.get('state',{}).get('note')=='temporary')
 h.engine.stop(kill=True);text('disconnected');h.engine.start();text('back online');r=wait(lambda r:r.get('connection') in ('back online',''));assert r['dirty'] and r['state']['screen']=='investigation';assert h.fixture.probes==2
 tap('Reload dashboard');r=wait(lambda r:not r.get('dirty',True) and r.get('state',{}).get('screen')=='overview' and r.get('state',{}).get('samples',{}).get('cpu',{}).get('value')==42 and len(r.get('state',{}).get('samples',{}))==9)
 result={'passed':True,'serial':serial,'checks':['native named resources','nine subscriptions','metrics/quality/age','independent Watch flags','investigation results','manual run','background collection','resume retains local state','restart no duplicate investigation','saved baseline reload'],'state':r}
finally:
 adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False)
 if h:adb('reverse','--remove',f'tcp:{h.port}',check=False)
 adb('shell','rm','-f','/sdcard/talia-p3.xml',check=False)
 if h:h.close()
assert not adb('shell','pm','path',package,check=False).strip();result['cleanup_verified']=True;print(json.dumps(result))
