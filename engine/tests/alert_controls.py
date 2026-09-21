import pathlib, tempfile, subprocess, json, os, time, urllib.request, xml.etree.ElementTree as ET, re
root=pathlib.Path(__file__).resolve().parents[2]; tmp=pathlib.Path(tempfile.mkdtemp(prefix='talia-alert-smoke-'));bin=pathlib.Path(os.environ.get('TALIA_BIN_DIR',str(root/'engine/target/debug')));serial=os.environ.get('TALIA_EMULATOR','emulator-5570');assert serial.startswith('emulator-'), 'Android tests require an emulator';pkg='com.lelloman.talia.dashboard';processes=[]
def rpc(op,args={}):
 req=urllib.request.Request(f'http://127.0.0.1:{port}/alerts',data=json.dumps({'op':op,'args':args}).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+token});r=json.load(urllib.request.urlopen(req));assert 'error' not in r,r;return r
def adb(*args,**kwargs):return subprocess.run(['adb','-s',serial,*args],check=True,capture_output=True,**kwargs).stdout
def click(text):
 for _ in range(15):
  adb('shell','uiautomator','dump','/sdcard/alerts-ui.xml');xml=ET.fromstring(adb('exec-out','cat','/sdcard/alerts-ui.xml'))
  for n in xml.iter('node'):
   if n.get('text','').lower()==text.lower():
    x1,y1,x2,y2=map(int,re.findall(r'\d+',n.get('bounds')));adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2));return
  time.sleep(.2)
 raise AssertionError('control missing: '+text)
assert not adb('shell','pm','path',pkg).strip(), 'Use a clean disposable emulator'
try:
 policy={'principal':'operator','expectedVersion':0,'enabled':True,'grants':[{'family':'alerts','actions':['read','acknowledge','silence','configure','observe','register','audit'],'scope':{'kind':'all'}}]};(tmp/'policy.json').write_text(json.dumps(policy));subprocess.run([str(bin/'talia-agent'),str(tmp/'db'),str(tmp/'policy.json'),str(tmp/'credential')],check=True,capture_output=True);token=(tmp/'credential').read_text()
 e=subprocess.Popen([str(bin/'talia-engine'),str(tmp/'db'),'0','--seed'],stdout=subprocess.PIPE);processes.append(e);engineport=json.loads(e.stdout.readline())['port']
 host=subprocess.Popen(['python3','dashboard/serve.py','0'],cwd=root,stdout=subprocess.PIPE,env={**os.environ,'TALIA_ENGINE_PORT':str(engineport),'TALIA_ENGINE_DB':str(tmp/'db')});processes.append(host);port=json.loads(host.stdout.readline())['port']
 rpc('observe',{'requestId':'open-web','expected':0,'observation':{'key':'web-alert','active':True,'stage':'warning','severity':'warning','message':'Disk low'}})
 js=tmp/'web.mjs';js.write_text('''import {chromium} from 'PLAYWRIGHT_PATH';import assert from 'node:assert/strict';const browser=await chromium.launch({headless:true});try{const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(String(e)));await page.goto(process.env.ALERT_URL);await page.evaluate(token=>sessionStorage.setItem('talia.alertCredential',token),process.env.ALERT_TOKEN);await page.locator('#alerts-panel summary').click();await page.getByRole('button',{name:'Acknowledge',exact:true}).click();await page.getByText('warning · Active · Acknowledged by operator',{exact:true}).waitFor();await page.getByRole('button',{name:'Silence for 1 hour'}).click();await page.getByRole('button',{name:'End silence'}).waitFor();await page.getByRole('button',{name:'End silence'}).click();await page.waitForFunction(()=>!document.querySelector('#alert-list').textContent.includes('End silence'));assert.deepEqual(errors,[]);console.log('web alert controls passed');}finally{await browser.close();}'''.replace('PLAYWRIGHT_PATH',str(root/'spikes/runtime/node_modules/playwright/index.mjs')))
 subprocess.run(['node',str(js)],check=True,env={**os.environ,'ALERT_URL':f'http://127.0.0.1:{port}','ALERT_TOKEN':token})
 rpc('observe',{'requestId':'open-native','expected':0,'observation':{'key':'native-alert','active':True,'stage':'warning','severity':'warning','message':'Native disk low'}})
 adb('install','-r',str(root/'dashboard/android/app/build/outputs/apk/debug/app-debug.apk'));adb('reverse',f'tcp:{port}',f'tcp:{port}')
 prefs=f'<?xml version="1.0"?><map><string name="credential">{token}</string></map>'.encode();adb('shell','run-as',pkg,'mkdir','-p','shared_prefs');adb('shell','run-as',pkg,'sh','-c',"'cat > shared_prefs/talia-alerts.xml'",input=prefs)
 adb('shell','am','start','-n',pkg+'/.MainActivity','--ei','port',str(port),'--ez','durable','true');click('Alerts');click('Acknowledge');
 for _ in range(30):
  if next(a for a in rpc('snapshot')['alerts'] if a['key']=='native-alert')['acknowledgement']:break
  time.sleep(.2)
 else:raise AssertionError('native acknowledgement missing')
 print('native alert controls passed')
finally:
 try:adb('shell','am','force-stop',pkg);adb('uninstall',pkg);adb('reverse','--remove',f'tcp:{port}')
 except Exception:pass
 for p in reversed(processes):p.terminate();p.wait(timeout=10)
 import shutil
 shutil.rmtree(tmp)
 print('alert control fixtures cleaned')
