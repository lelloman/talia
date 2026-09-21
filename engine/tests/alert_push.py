#!/usr/bin/env python3
"""Emulator-only registration/notification test. No real FCM credentials or delivery."""
import pathlib,tempfile,subprocess,json,os,time,urllib.request,shlex,sqlite3,shutil
root=pathlib.Path(__file__).resolve().parents[2];tmp=pathlib.Path(tempfile.mkdtemp(prefix='talia-push-test-'));bin=pathlib.Path(os.environ.get('TALIA_BIN_DIR',str(root/'engine/target/debug')));serial=os.environ.get('TALIA_EMULATOR','emulator-5570');assert serial.startswith('emulator-'),'Emulator required';pkg='com.lelloman.talia.dashboard';processes=[];port=None;installed=False

def adb(*args,**kwargs):return subprocess.run(['adb','-s',serial,*args],check=True,capture_output=True,**kwargs).stdout
def shell(*args,**kwargs):return adb('shell',shlex.join(args),**kwargs)
def rpc(op,args={}):
 req=urllib.request.Request(f'http://127.0.0.1:{port}/alerts',data=json.dumps({'op':op,'args':args}).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+token});r=json.load(urllib.request.urlopen(req));assert 'error' not in r,r;return r

def fixture(mode,override=None,**extra):
 shell('run-as',pkg,'rm','-f','files/alert-fixture.json')
 args=['am','start','-n',pkg+'/.AlertFixtureActivity','--es','mode',mode,'--ei','port',str(port if override is None else override)]
 for key,value in extra.items():args+=['--es',key,str(value)]
 shell(*args)
 for _ in range(80):
  r=subprocess.run(['adb','-s',serial,'exec-out','run-as',pkg,'cat','files/alert-fixture.json'],capture_output=True)
  if r.returncode==0:
   try:return json.loads(r.stdout)
   except ValueError:pass
  time.sleep(.1)
 raise AssertionError('fixture did not complete')
def observe(key,expected=0,active=True):return rpc('observe',{'requestId':key+'-'+str(expected),'expected':expected,'observation':{'key':key,'active':active,'stage':'warning','severity':'warning','message':'Fixture disk warning'}})['alert']
def data(alert,expiry=None):return json.dumps({'key':alert['key'],'occurrence':str(alert['occurrence']),'revision':str(alert['revision']),'active':str(alert['active']).lower(),'severity':alert['severity'],'message':alert['message'],'expires':str(expiry or int(time.time()*1000)+60000)})
assert adb('get-state').strip()==b'device', 'Emulator unavailable'
assert not subprocess.run(['adb','-s',serial,'shell','pm','path',pkg],capture_output=True).stdout.strip(),'Use a clean disposable emulator'
try:
 policy={'principal':'operator','expectedVersion':0,'enabled':True,'grants':[{'family':'alerts','actions':['read','acknowledge','silence','configure','observe','register','audit'],'scope':{'kind':'all'}}]};(tmp/'policy.json').write_text(json.dumps(policy));subprocess.run([str(bin/'talia-agent'),str(tmp/'db'),str(tmp/'policy.json'),str(tmp/'credential')],check=True,capture_output=True);token=(tmp/'credential').read_text()
 engine=subprocess.Popen([str(bin/'talia-engine'),str(tmp/'db'),'0','--seed'],stdout=subprocess.PIPE);processes.append(engine);engineport=json.loads(engine.stdout.readline())['port'];host=subprocess.Popen(['python3','dashboard/serve.py','0'],cwd=root,stdout=subprocess.PIPE,env={**os.environ,'TALIA_ENGINE_PORT':str(engineport),'TALIA_ENGINE_DB':str(tmp/'db')});processes.append(host);port=json.loads(host.stdout.readline())['port']
 adb('install','-r',str(root/'dashboard/android/app/build/outputs/apk/debug/app-debug.apk'));installed=True;adb('reverse',f'tcp:{port}',f'tcp:{port}');shell('pm','grant',pkg,'android.permission.POST_NOTIFICATIONS')
 prefs=f'<?xml version="1.0"?><map><string name="credential">{token}</string></map>'.encode();shell('run-as',pkg,'mkdir','-p','shared_prefs');shell('run-as',pkg,'sh','-c','cat > shared_prefs/talia-alerts.xml',input=prefs)
 first=fixture('register',token='fixture-token-one');assert first['status']=='registered',first;identity=first['installation']
 rotated=fixture('register',token='fixture-token-two');assert rotated['installation']==identity and rotated['deviceVersion']==first['deviceVersion']+1,rotated
 with sqlite3.connect(tmp/'db') as db:assert json.loads(db.execute("SELECT body FROM alert_entities WHERE kind='device' AND id=?",(identity,)).fetchone()[0])['token']=='fixture-token-two'
 shell('am','force-stop',pkg);assert fixture('inspect')['installation']==identity
 adb('install','-r',str(root/'dashboard/android/app/build/outputs/apk/debug/app-debug.apk'));assert fixture('inspect')['installation']==identity
 a=observe('push-alert');assert fixture('deliver',data=data(a,1))['notifications']==[]
 shown=fixture('deliver',data=data(a));assert len(shown['notifications'])==1 and shown['notifications'][0]['actions']==1,shown
 fixture('ack',key=a['key'])
 for _ in range(50):
  if next(x for x in rpc('snapshot')['alerts'] if x['key']==a['key'])['acknowledgement']:break
  time.sleep(.1)
 else:raise AssertionError('notification acknowledgement missing')
 assert not fixture('inspect')['notifications']
 offline=observe('offline-alert');fixture('deliver',override=1,data=data(offline));fixture('ack',key=offline['key'])
 for _ in range(20):
  result=fixture('inspect')
  if any(n['title']=='Acknowledgement failed' for n in result['notifications']):break
  time.sleep(.1)
 else:raise AssertionError('offline failure was not visible')
 assert next(x for x in rpc('snapshot')['alerts'] if x['key']==offline['key'])['acknowledgement'] is None
 resolved=observe('resolved-alert');fixture('deliver',data=data(resolved));observe('resolved-alert',resolved['revision'],False);fixture('open',key='resolved-alert')
 time.sleep(1);shell('uiautomator','dump','/sdcard/push-ui.xml');assert b'Resolved' in adb('exec-out','cat','/sdcard/push-ui.xml')
 shell('pm','clear',pkg);assert fixture('inspect')['installation']!=identity
 print(json.dumps({'passed':True,'checks':['registration','token rotation','restart/update identity','expiry','notification acknowledgement','offline acknowledgement failure','open fetches resolved state','clear-data creates new identity']}))
finally:
 if installed:
  try:shell('am','force-stop',pkg);adb('uninstall',pkg)
  except Exception:pass
 if port:
  try:adb('reverse','--remove',f'tcp:{port}')
  except Exception:pass
 for p in reversed(processes):p.terminate();p.wait(timeout=10)
 shutil.rmtree(tmp)
