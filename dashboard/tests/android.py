#!/usr/bin/env python3
"""Native UI smoke; always removes this test package and its reverse mapping."""
import json,subprocess,sys,time,re,xml.etree.ElementTree as ET
serial=sys.argv[1];package='com.lelloman.talia.dashboard'
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check).stdout
server=subprocess.Popen(['python3','dashboard/serve.py','0'],stdout=subprocess.PIPE,text=True)
port=json.loads(server.stdout.readline())['port']
def report():
 try:return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
 except Exception:return {}
def wait(predicate,timeout=20):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  r=report()
  if r.get('failure'):raise AssertionError(r)
  if predicate(r):return r
  time.sleep(.15)
 raise AssertionError(report())
def hierarchy():
 adb('shell','uiautomator','dump','/sdcard/talia-p1.xml');return ET.fromstring(adb('shell','cat','/sdcard/talia-p1.xml'))
def tap(label):
 for n in hierarchy().iter('node'):
  if n.get('text')==label or n.get('content-desc')==label:
   x1,y1,x2,y2=map(int,re.findall(r'\d+',n.get('bounds')));adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2));return
 raise AssertionError('Missing native control: '+label+' '+ET.tostring(hierarchy(),encoding='unicode'))
try:
 adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{port}',f'tcp:{port}');adb('shell','am','start','-n',package+'/.MainActivity','--ei','port',str(port))
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
 print(json.dumps({'passed':True,'serial':serial,'checks':['native controls, no WebView','switch and condition','server write','accessible chart','navigation retains VM/subscription'],'report':r}))
finally:
 adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False);adb('reverse','--remove',f'tcp:{port}',check=False);adb('shell','rm','-f','/sdcard/talia-p1.xml',check=False);server.terminate();server.wait(timeout=10)
