#!/usr/bin/env python3
"""Native UI smoke; always removes this test package and its reverse mapping."""
import json,subprocess,sys,time,re,xml.etree.ElementTree as ET,tempfile,pathlib,shlex,urllib.request
serial=sys.argv[1];package='com.lelloman.talia.dashboard'
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check).stdout
temp=tempfile.TemporaryDirectory(prefix='talia-native-updates-');definition=pathlib.Path(temp.name)/'package.json';pkg=json.loads(pathlib.Path('dashboard/generated/monitor.json').read_text());definition.write_text(json.dumps(pkg))
server=subprocess.Popen(['python3','dashboard/serve.py','0',str(definition)],stdout=subprocess.PIPE,text=True)
port=json.loads(server.stdout.readline())['port']
sequence=0
def rpc(op,args):
 global sequence
 sequence+=1
 request=urllib.request.Request(f'http://127.0.0.1:{port}/rpc',data=json.dumps(dict(session='p1-dashboard',channel='test',epoch=1,id=sequence,op=op,args=args)).encode(),headers={'Content-Type':'application/json'})
 with urllib.request.urlopen(request,timeout=6) as response:reply=json.load(response)
 assert 'error' not in reply,reply
 return reply['value']
def report():
 try:return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
 except Exception:return {}
def wait(predicate,timeout=20,allow_failure=False):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  r=report()
  if r.get('failure') and not allow_failure:raise AssertionError(r)
  if predicate(r):return r
  time.sleep(.15)
 raise AssertionError(report())
def hierarchy():
 adb('shell','uiautomator','dump','/sdcard/talia-p1.xml');return ET.fromstring(adb('shell','cat','/sdcard/talia-p1.xml'))
def foreground():
 adb('shell','am','start','-W','-n',package+'/.MainActivity')
 end=time.monotonic()+15
 while time.monotonic()<end:
  if any(n.get('package')==package and n.get('text') in {'Overview','Restart dashboard'} for n in hierarchy().iter('node')):return
  time.sleep(.2)
 raise AssertionError('Dashboard did not return to foreground')
def home():
 adb('shell','input','keyevent','KEYCODE_HOME')
 end=time.monotonic()+15
 while time.monotonic()<end:
  if not any(n.get('package')==package for n in hierarchy().iter('node')):return
  time.sleep(.2)
 raise AssertionError('Dashboard did not leave foreground')
def tap(label):
 end=time.monotonic()+15
 while time.monotonic()<end:
  for n in hierarchy().iter('node'):
   if n.get('package')==package and (n.get('text')==label or n.get('content-desc')==label):
    x1,y1,x2,y2=map(int,re.findall(r'\d+',n.get('bounds')));adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2));return
  time.sleep(.2)
 raise AssertionError('Missing native control: '+label)
def live(source):adb('shell','am','start','-W','-n',package+'/.MainActivity','--es','live',shlex.quote(source))
try:
 adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{port}',f'tcp:{port}');adb('shell','am','start','-n',package+'/.MainActivity','--ei','port',str(port),'--ez','failure_signals','true')
 wait(lambda r:len(r.get('state',{}).get('history',[]))>0)
 tap('Controls');wait(lambda r:r.get('state',{}).get('screen')=='controlsScreen')
 tap('Show details');wait(lambda r:r.get('state',{}).get('details') is False)
 tap('Side navigation');wait(lambda r:r.get('sidebar') is True);assert report()['state']['details'] is False
 tap('Side navigation');wait(lambda r:r.get('sidebar') is False)
 tap('Apply value');wait(lambda r:any(a.get('status')=='completed' for a in r.get('actions',[])))
 tap('Overview');r=wait(lambda r:r.get('state',{}).get('screen')=='overviewScreen' and 25 in r.get('state',{}).get('history',[]))
 xml=hierarchy();assert all('WebView' not in n.get('class','') for n in xml.iter('node'))
 assert any('Observed server values' in n.get('content-desc','') for n in xml.iter('node'))
 assert r['subscriptions']==1
 adb('shell','am','start','-n',package+'/.MainActivity','--ez','renderer_checks','true')
 renderer_result=None
 for _ in range(100):
  try:renderer_result=json.loads(adb('shell','run-as',package,'cat','files/renderer-checks.json'));break
  except Exception:time.sleep(.1)
 assert renderer_result and renderer_result['passed'],renderer_result
 live('void 0');wait(lambda r:r.get('dirty') is True)
 tap('Controls');wait(lambda r:r.get('state',{}).get('screen')=='controlsScreen')
 action=report()['nextActionId'];rpc('test',{'command':'hold','key':action});tap('Apply value');wait(lambda r:action in r.get('actionIds',[]))
 home();before=wait(lambda r:r.get('paused') is True and r.get('subscriptions')==0)
 rpc('test',{'command':'release','key':action})
 for _ in range(50):
  if rpc('status',{'actionId':action})['status']=='completed':break
  time.sleep(.1)
 assert rpc('status',{'actionId':action})['status']=='completed'
 assert report()['state']==before['state']
 foreground();wait(lambda r:not r.get('paused') and len(r.get('actions',[]))==2 and all(a.get('status')=='completed' for a in r.get('actions',[])) and not r.get('state',{}).get('busy'))
 assert report()['dirty'] is True
 for n in range(3):
  home();wait(lambda r:r.get('paused') is True)
  rpc('action',{'actionId':'external-'+str(n),'value':60+n})
  foreground();wait(lambda r:r.get('state',{}).get('history',[-1])[-1]==60+n and r.get('subscriptions')==1)
 for attempt in range(3):
  tap('Overview');time.sleep(.3)
  if report().get('state',{}).get('screen')=='overviewScreen':break
 wait(lambda r:r.get('state',{}).get('screen')=='overviewScreen')
 live("TaliaVM.replaceAction('controls',c=>{const s=c.state();c.commit(s,{...s.value,screen:'controlsScreen',details:false});});")
 wait(lambda r:r.get('dirty') is True)
 updated=json.loads(json.dumps(pkg));updated['revision']='revision-2';updated['definitions']['notice']['props']['text']='Updated shared notice';definition.write_text(json.dumps(updated))
 wait(lambda r:r.get('updateAvailable') is True);assert report()['definitionRevision']==pkg['revision']
 tap('Reload dashboard');wait(lambda r:r.get('definitionRevision')=='revision-2' and r.get('dirty') is False and 62 in r.get('state',{}).get('history',[]))
 assert any(n.get('text')=='Updated shared notice' for n in hierarchy().iter('node'))
 for fault_index,fault in enumerate(["throw Error('internal test failure')","while(true){}","globalThis.buffers=[];for(let i=0;i<100;i++)buffers.push(new ArrayBuffer(1024*1024));"]):
  live(fault);failed=wait(lambda r:bool(r.get('failure')),allow_failure=True);assert len(failed['signals'])==fault_index+1 and failed['subscriptions']==0
  home();foreground();assert report().get('failure')
  tap('Restart dashboard');r=wait(lambda r:not r.get('failure') and r.get('dirty') is False and 62 in r.get('state',{}).get('history',[]),allow_failure=True)
 selected=json.loads(json.dumps(updated));selected['id']='secondary';selected['revision']='client-selection';definition.write_text(json.dumps(selected));adb('shell','am','force-stop',package);adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',str(port),'--es','dashboard','secondary');r=wait(lambda r:r.get('dashboardId')=='secondary' and r.get('definitionRevision')=='client-selection' and 62 in r.get('state',{}).get('history',[]));adb('shell','am','force-stop',package);foreground();r=wait(lambda r:r.get('dashboardId')=='secondary' and 62 in r.get('state',{}).get('history',[]))
 result={'passed':True,'serial':serial,'checks':['independent persisted client dashboard selection','native controls, no WebView','switch and condition','server write','accessible chart','navigation retains VM/subscription','per-client composition','update available without adoption','reload adopts reference revision and discards dirty VM','exception, CPU and memory failures require manual restart and preserve server effects','actual background pauses local state and subscriptions','in-flight action completes in background','resume reconciles without resubmission','repeated background transitions retain dirty state'],'renderer':renderer_result,'report':r}
finally:
 adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False);adb('reverse','--remove',f'tcp:{port}',check=False);adb('shell','rm','-f','/sdcard/talia-p1.xml',check=False);server.terminate();server.wait(timeout=10);temp.cleanup()

result['cleanup_verified']=adb('get-state').strip()=='device' and not adb('shell','pm','path',package,check=False).strip() and f'tcp:{port}' not in adb('reverse','--list')
assert result['cleanup_verified']
print(json.dumps(result))
