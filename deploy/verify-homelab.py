#!/usr/bin/env python3
"""Authorized first-install verification: creates a diagnostic value and restarts Talìa.
Credentials stay in .local/deployment; output contains only verification evidence.
"""
import json,os,pathlib,queue,subprocess,threading,urllib.request,urllib.error
ROOT=pathlib.Path(__file__).resolve().parents[1]
ORIGIN='https://talia.lan.lelloman.com';private=ROOT/'.local/deployment'
proc=subprocess.Popen(['/tmp/talia-p3-target/debug/talia-mcp',ORIGIN,str(private/'operator.token')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
messages=queue.Queue()
def receive():
 for line in proc.stdout:messages.put(json.loads(line))
threading.Thread(target=receive,daemon=True).start();seq=0
checks=[]
def rpc(method,params):
 global seq
 seq+=1;proc.stdin.write(json.dumps({'jsonrpc':'2.0','id':seq,'method':method,'params':params})+'\n');proc.stdin.flush()
 while True:
  reply=messages.get(timeout=40)
  if reply.get('id')==seq:assert 'error' not in reply,reply;return reply['result']
def tool(name,args):
 r=rpc('tools/call',{'name':name,'arguments':args});value=r.get('structuredContent') or json.loads(r['content'][0]['text']);assert not r.get('isError'),value;return value
try:
 rpc('initialize',{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'homelab-deployment-check','version':'1'}})
 proc.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'})+'\n');proc.stdin.flush()
 assert any(t['name']=='engine_write' for t in rpc('tools/list',{})['tools'])
 checks.append('MCP stdio adapter connects over verified private HTTPS')
 listing=tool('definitions_list',{})
 def put(kind,id,document,**kw):return {'op':'put','key':{'kind':kind,'id':id},'document':document,**kw}
 empty={'version':1,'value':['object',[]]}
 changes=[put('variable_definition','deployment-check-type',dict(id='deployment-check-type',version=1,kind='stored',source='',value_schema='any',state_schema='any',dependencies=[])),put('variable','deployment-check',dict(id='deployment-check',definition='deployment-check-type',params=empty,history_count=10,history_age_ms=86400000),initial={'state':empty})]
 if listing['catalogRevision']==1:tool('definitions_save',{'requestId':'first-deployment-check-definition','changeSet':{'expectedCatalogRevision':listing['catalogRevision'],'changes':changes}})
 before=tool('engine_read',{'id':'deployment-check'})['sample']
 expected={'version':1,'value':['string','First deployment persistence verified']}
 if before['value']!=expected:tool('engine_write',dict(id='deployment-check',expectedRevision=before['revision'],value=expected,requestId='first-deployment-check-write'))
 checks.append('authenticated definition creation and persisted diagnostic write')
 js=r"""
const fs=require('fs'),{execFileSync}=require('child_process');
const {chromium}=require(process.cwd()+'/spikes/runtime/node_modules/playwright');
(async()=>{const browser=await chromium.launch({headless:true,args:['--no-sandbox']});try{
 const context=await browser.newContext();const page=await context.newPage();await page.goto(process.env.ORIGIN);
 await page.locator('input').fill(fs.readFileSync('.local/deployment/access.token','utf8').trim());await page.getByRole('button',{name:'Sign in'}).click();
 await page.getByText('Talìa is online',{exact:true}).waitFor();
 await page.waitForFunction(()=>window.dashboardReport?.registration?.clientId);
 const client=await page.evaluate(()=>window.dashboardReport.registration.clientId);
 const hello=()=>page.evaluate(async()=>await (await fetch('/engine',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({version:1,client:'deployment-browser-check',epoch:1,op:'hello'})})).json());
 const before=await hello();if(before.error)throw Error(before.error);
 execFileSync('ssh',['homelab','docker compose -f /home/lelloman/homelab/talia/docker-compose.yml restart talia'],{stdio:'pipe'});
 await page.waitForFunction(async old=>{try{const r=await (await fetch('/engine',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({version:1,client:'deployment-browser-check',epoch:1,op:'hello'})})).json();return r.incarnation!==old&&!r.error;}catch{return false;}},before.incarnation,{timeout:30000});
 await page.reload();await page.getByText('Talìa is online',{exact:true}).waitFor();await page.waitForFunction(id=>window.dashboardReport?.registration?.clientId===id,client);
 console.log('Verified TLS, browser sign-in, welcome dashboard, restart and retained client registration');
}finally{await browser.close();}})().catch(e=>{console.error(e);process.exit(1)});
"""
 subprocess.run(['node','-e',js],cwd=ROOT,env=dict(os.environ,ORIGIN=ORIGIN),check=True)
 assert tool('engine_read',{'id':'deployment-check'})['sample']['value']==expected
 checks.extend(['browser sign-in and welcome UI with trusted TLS','service restart and retained browser registration','MCP reconnect and diagnostic value retained after restart'])
 for path in ['/engine','/clients']:
  try:urllib.request.urlopen(urllib.request.Request(ORIGIN+path,data=b'{}',headers={'Content-Type':'application/json'}));raise AssertionError('unauthenticated endpoint accepted')
  except urllib.error.HTTPError as e:assert e.code==401
 checks.append('unauthenticated engine and registration denied')
 report={'origin':ORIGIN,'checks':checks,'passed':True,'sourceRevision':'dee6dd2dc5ab229f9dbc2902ce38823acee3e168','imageDigest':'sha256:6ae1f36e696a4e34b0b38bd1f6421b0f3d3cdf92dc717d02542e27d83d24ba7d','homelabCommit':'1989f1d','providerDeliveryTested':False,'androidDeviceUsed':False}
 (ROOT/'deploy/first-deployment.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
finally:
 proc.stdin.close();proc.wait(timeout=10)
