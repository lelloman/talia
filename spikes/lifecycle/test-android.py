#!/usr/bin/env python3
import subprocess,json,time,urllib.request,sys,re,xml.etree.ElementTree as ET
from pathlib import Path
serial=sys.argv[1];port=int(sys.argv[2]);package='com.lelloman.talia.spike';seq=0
checks=[];run=str(time.time_ns())
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check,timeout=60).stdout.strip()
def rpc(op,args):
 global seq;seq+=1
 body=json.dumps(dict(session='lifecycle-android',channel='test',epoch=1,id=seq,op=op,args=args)).encode()
 result=json.load(urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/rpc',body,{'Content-Type':'application/json'}),timeout=10));assert 'error' not in result,result;return result['value']
def report():return json.loads(adb('shell','run-as',package,'cat','/data/user_de/0/'+package+'/files/lifecycle.json'))
def until(predicate):
 for _ in range(100):
  try:
   r=report()
   if predicate(r):return r
  except (ValueError,subprocess.CalledProcessError):pass
  time.sleep(.1)
 raise AssertionError('condition timeout: '+str(report()))
def start(command=0):adb('shell','am','start','-W','-n',package+'/.LifecycleActivity','--ei','transport_port',str(port),'--ei','command',str(command),'--ez','failure_signals','true')
try:
 adb('reverse',f'tcp:{port}',f'tcp:{port}')
 adb('install','-r','spikes/runtime/android/app/build/outputs/apk/debug/app-debug.apk');start()
 until(lambda r:r['active'] and r['snapshot'] is not None)
 start(1);until(lambda r:r['local']['dirty']);checks.append('temporary edit')
 pid=adb('shell','pidof',package);action=f'a-{pid}-1-1';rpc('test',{'command':'hold','key':action})
 start(2);until(lambda r:action in r['outcomes']);
 assert rpc('status',{'actionId':action})['status']=='accepted'
 adb('shell','input','keyevent','KEYCODE_HOME');paused=until(lambda r:not r['active']);ticks=paused['local']['ticks']
 rpc('test',{'command':'release','key':action})
 time.sleep(.4);assert rpc('status',{'actionId':action})['status']=='completed'
 r=report();assert r['local']['ticks']==ticks and r['subscriptions']==0;checks.append('pause during dispatched action; server continues')
 start();resumed=until(lambda r:r['active'] and r['snapshot']['value']==42 and r['outcomes'].get(action,{}).get('status')=='completed')
 assert resumed['local']['dirty'] and len(resumed['outcomes'])==1;checks.append('resume reconciles without resubmission; dirty retained')
 for n in range(3):
  adb('shell','input','keyevent','KEYCODE_HOME');until(lambda r:not r['active']);rpc('action',{'actionId':'external-'+run+'-'+str(n),'value':60+n});start();until(lambda r:r['active'] and r['snapshot']['value']==60+n and r['subscriptions']==1)
 checks.append('repeated transitions refresh without duplicate subscriptions')
 adb('shell','am','force-stop',package);start();fresh=until(lambda r:r['active'] and not r['local']['dirty'] and r['snapshot'] is not None and r['snapshot']['value']==62)
 checks.append('process recreation restores saved baseline and server effects')
 start(7);until(lambda r:r['external_error'] and r['active'] and not r['failure']);checks.append('external failure is recoverable')
 for command in (4,5,6):
  start(1);until(lambda r:r['local']['dirty']);count=len(report()['signals'])
  start(command);failed=until(lambda r:r['failure'] is not None)
  assert not failed['active'] and failed['subscriptions']==0 and failed['pending_waits']==0 and len(failed['signals'])==count+1
  ticks=failed['survivor_ticks'];until(lambda r:r['survivor_ticks']>ticks)
  adb('shell','uiautomator','dump','/sdcard/talia-window.xml');ui=adb('shell','cat','/sdcard/talia-window.xml');adb('shell','rm','/sdcard/talia-window.xml');Path('/tmp/talia-failure-ui.xml').write_text(ui);assert 'restart dashboard' in ui.lower() and 'Dashboard stopped' in ui
  adb('shell','input','keyevent','KEYCODE_HOME');time.sleep(.2);start();assert report()['failure'] is not None
  checks.append(str(command)+' visible stopped state, signal, cleanup, survivor progress and no resume restart')
  adb('shell','uiautomator','dump','/sdcard/talia-window.xml');ui=adb('shell','cat','/sdcard/talia-window.xml');adb('shell','rm','/sdcard/talia-window.xml')
  button=next(n for n in ET.fromstring(ui).iter('node') if n.get('text','').lower()=='restart dashboard');bounds=list(map(int,re.findall(r'\d+',button.attrib['bounds'])));adb('shell','input','tap',str((bounds[0]+bounds[2])//2),str((bounds[1]+bounds[3])//2));fresh=until(lambda r:r['active'] and not r['failure'] and not r['local']['dirty'] and r['snapshot'] is not None and r['snapshot']['value']==62)
  checks.append(str(command)+' manual restart preserves server effects')
 print(json.dumps({'passed':True,'checks':checks,'final':fresh,'sdk':adb('shell','getprop','ro.build.version.sdk'),'model':adb('shell','getprop','ro.product.model')}))
finally:
 adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False);adb('reverse','--remove',f'tcp:{port}',check=False)
 assert not adb('shell','pm','list','packages',package)
 assert not adb('shell','pidof',package,check=False)
