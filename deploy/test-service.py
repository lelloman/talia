#!/usr/bin/env python3
import ssl,threading,http.server,http.client
import json,os,pathlib,secrets,subprocess,tempfile,urllib.request,urllib.error
ROOT=pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='talia-deployment-') as tmp:
 p=pathlib.Path(tmp);token=secrets.token_hex(32);(p/'token').write_text(token)
 subprocess.run(['python3','deploy/package-web.py',str(p/'web')],cwd=ROOT,check=True)
 class Proxy(http.server.BaseHTTPRequestHandler):
  def handle_request(self):
   upstream=http.client.HTTPConnection('127.0.0.1',port,timeout=10)
   try:
    data=self.rfile.read(int(self.headers.get('Content-Length','0')))
    upstream.request(self.command,self.path,data,dict(self.headers));r=upstream.getresponse();body=r.read();self.send_response(r.status)
    for k,v in r.getheaders():
     if k.lower() not in ['transfer-encoding','connection','content-length']:self.send_header(k,v)
    self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
   except (BrokenPipeError,ConnectionResetError,ssl.SSLEOFError):pass
   finally:upstream.close()
  do_GET=do_POST=handle_request
  def log_message(self,*a):pass
 proxy=http.server.ThreadingHTTPServer(('127.0.0.1',0),Proxy)
 subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(p/'key'),'-out',str(p/'cert'),'-days','1','-subj','/CN=localhost'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);tls.load_cert_chain(p/'cert',p/'key');proxy.socket=tls.wrap_socket(proxy.socket,server_side=True)
 threading.Thread(target=proxy.serve_forever,daemon=True).start()
 origin='https://localhost:'+str(proxy.server_port)
 env=dict(os.environ,TALIA_ACCESS_TOKEN_FILE=str(p/'token'),TALIA_WEB_ROOT=str(p/'web'),TALIA_PUBLIC_ORIGIN=origin,TALIA_LISTEN='127.0.0.1')
 command=[os.environ.get('TALIA_BIN','/tmp/talia-p3-target/debug/talia-engine'),str(p/'state.sqlite3'),'0']
 bootstrap=str(pathlib.Path(command[0]).with_name('talia-bootstrap'))
 subprocess.run([bootstrap,command[1]],check=True)
 assert subprocess.run([bootstrap,command[1]],capture_output=True).returncode!=0
 def start():
  proc=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,text=True);port=json.loads(proc.stdout.readline())['port'];return proc,port
 proc,port=start()
 def request(path,body=None,headers={}):
  data=None if body is None else json.dumps(body).encode()
  try:
   with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=data,headers={'Content-Type':'application/json',**headers}),timeout=10) as r:return r.status,r.read(),r.headers
  except urllib.error.HTTPError as e:return e.code,e.read(),e.headers
 try:
  assert request('/healthz')[0]==200
  for path in ['/engine','/clients']:assert request(path,{'version':1,'client':'deployment-test','epoch':1,'op':'hello'})[0]==401
  assert b'Deployment access key' in request('/')[1]
  for path in ['/.git/config','/engine/src/main.rs','/deploy/login.html','/../token','/%2e%2e/token']:assert request(path)[0]==404,path
  assert request('/session',{'token':'wrong'})[0]==401
  assert request('/session',{'token':token},{'Origin':'https://evil.test'})[0]==403
  status,body,h=request('/session',{'token':token},{'Origin':origin});assert status==204
  cookie=h['Set-Cookie'];assert all(x in cookie for x in ['Secure','HttpOnly','SameSite=Strict','Path=/'])
  access={'Cookie':cookie.split(';')[0]}
  assert b'id="dashboard"' in request('/',headers=access)[1]
  assert request('/engine',{'version':1,'client':'deployment-test','epoch':1,'op':'hello'},access)[0]==200
  assert request('/engine',{'version':1,'client':'deployment-test','epoch':1,'op':'hello'},{**access,'Origin':'https://evil.test'})[0]==403
  value=json.loads(request('/engine',{'version':1,'client':'deployment-test','epoch':1,'op':'hello'},access)[1]);assert 'value' in value,value
  assert 'value' in json.loads(request('/clients',{'op':'register','name':'Deployment test','platform':'web'},{**access,'Authorization':'Bearer '+'a'*64})[1])
  proc.terminate();proc.wait(timeout=10);proc,port=start()
  assert request('/healthz')[0]==200
  after=json.loads(request('/engine',{'version':1,'client':'deployment-test','epoch':1,'op':'hello'},access)[1]);assert after['value']['values']==value['value']['values']
  # Authenticated requests can use a host-only header (native clients/operators).
  assert request('/engine',{'version':1,'client':'deployment-test','epoch':1,'op':'hello'},{'X-Talia-Access':token})[0]==200
  browser_script=r"""
const {chromium}=require(process.cwd()+'/spikes/runtime/node_modules/playwright');
(async()=>{const b=await chromium.launch({headless:true,args:['--no-sandbox']});try{
 const context=await b.newContext({ignoreHTTPSErrors:true});
 const page=await context.newPage();await page.goto(process.env.TEST_ORIGIN+'/');await page.locator('input').fill(process.env.TEST_TOKEN);await page.getByRole('button',{name:'Sign in'}).click();
 await page.waitForFunction(()=>window.dashboardReport?.stateWire && !window.dashboardReport?.failure,{},{timeout:20000});
 console.log('authenticated web dashboard rendered');
}finally{await b.close();}})().catch(e=>{console.error(e);process.exit(1)});
"""
  subprocess.run(['node','-e',browser_script],cwd=ROOT,env=dict(os.environ,TEST_TOKEN=token,TEST_ORIGIN=origin),check=True)
  print('deployment access, origin protection, assets, registration, readiness and restart passed')
 finally:proc.terminate();proc.wait(timeout=10);proxy.shutdown();proxy.server_close()
