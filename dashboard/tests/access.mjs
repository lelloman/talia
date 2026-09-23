// Real browser + actual Rust service, with a local OIDC provider (test binary only).
import {Client} from '../web/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import {StreamableHTTPClientTransport} from '../web/node_modules/@modelcontextprotocol/sdk/dist/esm/client/streamableHttp.js';
import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {spawn,execFileSync} from 'node:child_process';import {once} from 'node:events';
import {mkdirSync,mkdtempSync,writeFileSync,readFileSync,rmSync} from 'node:fs';import os from 'node:os';import path from 'node:path';import http from 'node:http';import https from 'node:https';import crypto from 'node:crypto';import assert from 'node:assert/strict';
const tmp=mkdtempSync(path.join(os.tmpdir(),'talia-access-'));let engine,browser,proxy,provider;
try{
 execFileSync('openssl',['req','-x509','-newkey','rsa:2048','-nodes','-keyout',tmp+'/tls.key','-out',tmp+'/tls.crt','-days','1','-subj','/CN=localhost','-addext','subjectAltName=DNS:localhost'],{stdio:'ignore'});
 const keys=crypto.generateKeyPairSync('rsa',{modulusLength:2048}),jwk={...keys.publicKey.export({format:'jwk'}),kid:'fixture',use:'sig',alg:'RS256'};
 let issuer,origin,port;const codes=new Map(),tokens=new Map(),refreshTokens=new Map(),refreshCounts=new Map();
 const json=(res,v)=>{res.setHeader('Content-Type','application/json');res.end(JSON.stringify(v));};
 provider=http.createServer(async(req,res)=>{
  const u=new URL(req.url,issuer);if(u.pathname==='/.well-known/openid-configuration')return json(res,{issuer,authorization_endpoint:issuer+'/authorize',token_endpoint:issuer+'/token',jwks_uri:issuer+'/jwks',introspection_endpoint:issuer+'/introspect',response_types_supported:['code'],id_token_signing_alg_values_supported:['RS256'],code_challenge_methods_supported:['S256'],token_endpoint_auth_methods_supported:['none','client_secret_basic']});
  if(u.pathname==='/jwks')return json(res,{keys:[jwk]});
  if(u.pathname==='/authorize'){const user=(req.headers.cookie||'').includes('fixture_admin=1')?'admin':'viewer';const code=crypto.randomUUID();codes.set(code,{user,nonce:u.searchParams.get('nonce'),challenge:u.searchParams.get('code_challenge')});const redirect=new URL(u.searchParams.get('redirect_uri'));redirect.searchParams.set('code',code);redirect.searchParams.set('state',u.searchParams.get('state'));res.writeHead(302,{Location:redirect.href});return res.end();}
  const chunks=[];for await(const c of req)chunks.push(c);const form=new URLSearchParams(Buffer.concat(chunks).toString());
  if(req.headers.authorization!=='Basic '+Buffer.from('talia:fixture-secret-with-at-least-thirty-two-characters').toString('base64')){res.statusCode=401;return res.end();}
  if(u.pathname==='/token'){
   const refreshing=form.get('grant_type')==='refresh_token';
   const c=refreshing?refreshTokens.get(form.get('refresh_token')):codes.get(form.get('code'));
   if(refreshing)refreshTokens.delete(form.get('refresh_token'));else codes.delete(form.get('code'));
   if(!c||(!refreshing&&crypto.createHash('sha256').update(form.get('code_verifier')||'').digest('base64url')!==c.challenge)){res.statusCode=400;return json(res,{error:'invalid_grant'});}
   if(refreshing)refreshCounts.set(c.user,(refreshCounts.get(c.user)||0)+1);
   const now=Math.floor(Date.now()/1000),ttl=refreshing?3600:5,payload={iss:issuer,sub:c.user,aud:'talia',iat:now,exp:now+ttl,name:c.user,...(refreshing?{}:{nonce:c.nonce})};
   const input=Buffer.from(JSON.stringify({alg:'RS256',kid:'fixture'})).toString('base64url')+'.'+Buffer.from(JSON.stringify(payload)).toString('base64url');
   const token=crypto.randomUUID(),refresh=crypto.randomUUID();tokens.set(token,c.user);refreshTokens.set(refresh,c);
   return json(res,{access_token:token,refresh_token:refresh,expires_in:ttl,token_type:'Bearer',id_token:input+'.'+crypto.sign('RSA-SHA256',Buffer.from(input),keys.privateKey).toString('base64url')});
  }
  if(u.pathname==='/introspect'){const sub=tokens.get(form.get('token'));return json(res,{active:!!sub,sub,iss:issuer,client_id:'talia'});}res.statusCode=404;res.end();
 });provider.listen(0,'127.0.0.1');await once(provider,'listening');issuer='http://127.0.0.1:'+provider.address().port;
 proxy=https.createServer({key:readFileSync(tmp+'/tls.key'),cert:readFileSync(tmp+'/tls.crt')},(req,res)=>{const upstream=http.request({hostname:'127.0.0.1',port,path:req.url,method:req.method,headers:req.headers},r=>{res.writeHead(r.statusCode,r.headers);r.pipe(res);});upstream.on('error',()=>{res.statusCode=503;res.end();});req.pipe(upstream);});proxy.listen(0,'127.0.0.1');await once(proxy,'listening');origin='https://localhost:'+proxy.address().port;
 execFileSync('openssl',['ecparam','-name','prime256v1','-genkey','-noout','-out',tmp+'/vapid.pem']);
 writeFileSync(tmp+'/providers.json',JSON.stringify({browser:{kind:'web_push',private_key:tmp+'/vapid.pem',subject:'mailto:fixture@example.test'}}));
 writeFileSync(tmp+'/secret','fixture-secret-with-at-least-thirty-two-characters');execFileSync('python3',['deploy/package-web.py',tmp+'/web']);
 engine=spawn('cargo',['test','--manifest-path','engine/Cargo.toml','--offline','--bin','talia-engine','browser_fixture_service','--','--ignored','--nocapture'],{env:{...process.env,TALIA_ALERT_PROVIDERS:tmp+'/providers.json',TALIA_TEST_DB:tmp+'/engine.db',TALIA_OIDC_ISSUER:issuer,TALIA_OIDC_CLIENT_ID:'talia',TALIA_OIDC_SECRET_FILE:tmp+'/secret',TALIA_PUBLIC_ORIGIN:origin,TALIA_AUTH_DB:tmp+'/auth.db',TALIA_WEB_ROOT:tmp+'/web',TALIA_BOOTSTRAP_ADMINS:issuer+'#admin'},detached:true,stdio:['ignore','pipe','pipe']});
 await new Promise((resolve,reject)=>{let out='';const timeout=setTimeout(()=>reject(Error('service startup timeout')),60000);engine.stdout.on('data',d=>{out+=d;const m=out.match(/\{"incarnation":"[^"]+","port":(\d+)\}/);if(m){port=Number(m[1]);clearTimeout(timeout);resolve();}});engine.stderr.on('data',d=>process.stderr.write(d));engine.on('exit',code=>{clearTimeout(timeout);reject(Error('service exited '+code));});});
 browser=await chromium.launch({headless:true,channel:'chromium',args:['--ignore-certificate-errors']});const admin=await browser.newContext({ignoreHTTPSErrors:true}),viewer=await browser.newContext({ignoreHTTPSErrors:true});await admin.addCookies([{name:'fixture_admin',value:'1',url:issuer}]);
 async function login(context){const p=await context.newPage();await p.goto(origin);await p.getByRole('link',{name:'Sign in with LelloAuth'}).click();await p.waitForFunction(()=>['Administrator','Viewer'].includes(document.querySelector('#account-role')?.textContent));return p;}
 const a=await login(admin),v=await login(viewer);await v.getByRole('heading',{name:'No dashboards available yet'}).waitFor();assert.equal(await v.locator('#sharing').isVisible(),false);
 const api=(p,body)=>p.evaluate(async body=>(await(await fetch('/account',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)})).json()),body);
 // Actual HTTPS worker and server registration; only the vendor push subscription is a fixture.
 await admin.grantPermissions(['notifications'],{origin});
 const pushKey=crypto.createECDH('prime256v1');pushKey.generateKeys();
 const pushSubscription={endpoint:'https://fcm.googleapis.com/fixture/opaque-secret',keys:{p256dh:pushKey.getPublicKey().toString('base64url'),auth:crypto.randomBytes(16).toString('base64url')}};
 await a.evaluate(subscription=>{
  let current=null;
  PushManager.prototype.getSubscription=async()=>current;
  PushManager.prototype.subscribe=async options=>{if(!options.userVisibleOnly)throw Error('must be visible');current={toJSON:()=>subscription,unsubscribe:async()=>{window.pushUnsubscribed=(window.pushUnsubscribed||0)+1;current=null;return true;}};return current;};
 },pushSubscription);
 await a.locator('.lv-sidebar a[href="#settings"]').click();
 await a.evaluate(()=>window.dispatchEvent(new Event('hashchange')));
 await a.waitForFunction(()=>!document.querySelector('#browser-push-enable').disabled).catch(async e=>{console.error(await a.locator('#browser-push-status').textContent());console.error(await api(a,{op:'browserPushConfig'}));throw e;});
 await a.locator('#browser-push-enable').click();
 await a.getByText('Browser notifications enabled.',{exact:true}).waitFor();
 const browserId=await a.evaluate(()=>JSON.parse(localStorage.getItem('talia.browserPush')).id);
 assert.match(browserId,/^browser-/);
 assert.equal((await api(a,{op:'browserPushStatus',id:browserId})).device.enabled,true);
 // Simulate an expired subscription retired server-side while the browser still caches it.
 await api(a,{op:'browserPushDisable',id:browserId});
 await a.evaluate(()=>window.dispatchEvent(new Event('hashchange')));
 await a.waitForFunction(()=>!document.querySelector('#browser-push-enable').disabled);
 await a.locator('#browser-push-enable').click();await a.getByText('Browser notifications enabled.',{exact:true}).waitFor();
 assert.equal(await a.evaluate(()=>window.pushUnsubscribed),1);
 assert.equal(await a.evaluate(()=>JSON.parse(localStorage.getItem('talia.browserPush')).id),browserId);
 assert.equal((await api(v,{op:'browserPushStatus',id:browserId})).error,'forbidden');
 assert.equal((await api(v,{op:'browserPushConfig'})).error,'forbidden');
 const browserConfig=await api(a,{op:'browserPushConfig'});
 assert.equal((await api(a,{op:'browserPushRegister',id:browserId,applicationServerKey:browserConfig.applicationServerKey,subscription:{...pushSubscription,endpoint:'http://127.0.0.1/private'}})).error,'push endpoint origin is not allowed');
 assert.equal(await a.evaluate(async()=>new URL((await navigator.serviceWorker.ready).active.scriptURL).pathname),'/push-sw.js');
 await a.locator('#browser-push-disable').click();
 await a.getByText('Browser notifications are disabled.',{exact:true}).waitFor();
 assert.equal((await api(a,{op:'browserPushStatus',id:browserId})).device.enabled,false);
 // Create a one-time key through the actual browser UI, then use the HTTP transport.
 await admin.grantPermissions(['clipboard-read','clipboard-write','notifications'],{origin});await a.bringToFront();
 await a.locator('.lv-sidebar a[href="#settings"]').click();await a.locator('#agent-key-create').click();
 await a.waitForFunction(()=>document.querySelector('#agent-key-value').value.length===64);
 const agentKey=await a.locator('#agent-key-value').inputValue();assert.equal(await a.locator('#agent-endpoint').inputValue(),origin+'/mcp');
 await a.locator('#agent-config-copy').click();await a.getByText('MCP configuration copied.',{exact:true}).waitFor();
 mkdirSync('.local/agent-access',{recursive:true});await a.screenshot({path:'.local/agent-access/settings.png',fullPage:true});
 const config=JSON.parse(await a.evaluate(()=>navigator.clipboard.readText()));assert.equal(config.mcpServers.talia.headers.Authorization,'Bearer '+agentKey);
 const keyList=await api(a,{op:'agentKeys'});const keyId=keyList.keys[0].id;assert.equal(keyList.keys[0].expires-keyList.keys[0].created,3600000);assert.equal(JSON.stringify(keyList).includes(agentKey),false);
 const mcp=async(key,method,params={},extra={})=>admin.request.post(origin+'/mcp',{headers:{Authorization:'Bearer '+key,Accept:'application/json, text/event-stream','MCP-Protocol-Version':'2025-11-25',...extra},data:{jsonrpc:'2.0',id:1,method,params}});
 assert.equal((await mcp('bad','tools/list')).status(),401);
 assert.equal((await admin.request.post(origin+'/mcp',{data:{jsonrpc:'2.0',id:1,method:'tools/list'}})).status(),401);
 assert.equal((await mcp(agentKey,'initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'test',version:'1'}})).status(),200);
 // Independent official TypeScript SDK against HTTPS with the fixture CA trusted.
 const fixtureFetch=(url,init={})=>new Promise((resolve,reject)=>{const r=https.request(url,{method:init.method||'GET',headers:Object.fromEntries(new Headers(init.headers)),ca:readFileSync(tmp+'/tls.crt'),signal:init.signal},res=>{const chunks=[];res.on('data',c=>chunks.push(c));res.on('end',()=>resolve(new Response(res.statusCode===204?null:Buffer.concat(chunks),{status:res.statusCode,headers:res.headers})));});r.on('error',reject);r.end(init.body);});
 const sdk=new Client({name:'talia-interoperability-test',version:'1'},{capabilities:{}});
 await sdk.connect(new StreamableHTTPClientTransport(new URL(origin+'/mcp'),{requestInit:{headers:{Authorization:'Bearer '+agentKey}},fetch:fixtureFetch}));
 assert.ok((await sdk.listTools()).tools.some(t=>t.name==='definitions_save'));
 const definitions=await sdk.callTool({name:'definitions_list',arguments:{}});assert.equal(definitions.isError,false);
 const catalogBody=definitions.structuredContent;
 const saved=await sdk.callTool({name:'definitions_save',arguments:{requestId:crypto.randomUUID(),changeSet:{expectedCatalogRevision:catalogBody.catalogRevision,changes:[{op:'put',key:{kind:'ui',id:'agent-test-notice'},document:{source:'<Text id="Notice" text="Agent-authored"/>'}}]}}});
 assert.equal(saved.isError,false);assert.equal(saved.structuredContent.audit.principal,issuer+'#admin');
 // Engine and host-routed tools must work through the same official HTTP SDK.
 const sdkTool=async(name,args={})=>{const r=await sdk.callTool({name,arguments:args});assert.equal(r.isError,false,JSON.stringify(r));return r.structuredContent;};
 const pushConfiguration=await sdkTool('alerts_config');
 assert.equal(pushConfiguration.destinations.find(d=>d.id===browserId).channel,'web_push');
 assert.equal(JSON.stringify(pushConfiguration).includes('opaque-secret'),false);
 await sdkTool('alerts_observe',{requestId:crypto.randomUUID(),expected:0,observation:{key:'browser-fixture',active:true,stage:'warning',severity:'warning',message:'Browser fixture alert'}});
 const pushAlert=(await sdkTool('alerts_snapshot')).alerts.find(alert=>alert.key==='browser-fixture');
 await api(a,{op:'browserPushRegister',id:browserId,subscription:pushSubscription,applicationServerKey:browserConfig.applicationServerKey});
 const pushWorker=admin.serviceWorkers().find(w=>w.url().endsWith('/push-sw.js'));
 const workerRequest={op:'browserPushAlert',id:browserId,key:pushAlert.key,occurrence:pushAlert.occurrence,revision:pushAlert.revision};
 const workerRead=body=>pushWorker.evaluate(async body=>(await fetch('/account',{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)})).json(),body);
 assert.equal((await workerRead(workerRequest)).alert.message,'Browser fixture alert');
 assert.equal((await workerRead({...workerRequest,revision:pushAlert.revision+1})).error,'notification superseded');
 await api(a,{op:'browserPushDisable',id:browserId});
 assert.equal((await workerRead(workerRequest)).error,'forbidden');
 // Reports are authored and executed server-side through the official HTTP MCP client.
 await sdkTool('reports_save',{requestId:crypto.randomUUID(),expected:0,definition:{id:'morning-fixture',version:1,enabled:false,schedule:null,steps:[{id:'sample',kind:'script',source:'ctx=>({status:"healthy"})'}],compose:'ctx=>({subject:"Morning fixture",summary:ctx.steps.sample.value.status,sections:[]})',destinations:[]}});
 const reportStart=await sdkTool('reports_run',{id:'morning-fixture',send:false,requestId:crypto.randomUUID()});
 let reportRun;for(let attempt=0;attempt<30;attempt++){reportRun=(await sdkTool('reports_run_get',{id:reportStart.run_id})).run;if(reportRun.status==='complete')break;assert.notEqual(reportRun.status,'failed',JSON.stringify(reportRun));await new Promise(r=>setTimeout(r,200));}
 assert.equal(reportRun.status,'complete');assert.equal(reportRun.content.summary,'healthy');assert.match(reportRun.html,/<h1>Morning fixture<\/h1>/);assert.equal(reportRun.deliveries.length,0);
 assert.equal((await sdkTool('reports_runs',{report:'morning-fixture'})).runs.length,1);
 // Observer service credentials never inherit the browser administrator's editing tools.
 await a.locator('#telegram-settings').evaluate(e=>e.open=true);
 await a.locator('#telegram-observer-key').click();
 await a.waitForFunction(()=>document.querySelector('#telegram-key').value.startsWith('to_'));
 const observerKey=await a.locator('#telegram-key').inputValue();
 const observerTools=(await(await mcp(observerKey,'tools/list')).json()).result.tools;
 assert.deepEqual(observerTools.map(t=>t.name),['observer_snapshot','observer_read','observer_probe']);
 const observation=(await(await mcp(observerKey,'tools/call',{name:'observer_snapshot',arguments:{}})).json()).result;
 assert.equal(observation.isError,false);
 assert.ok((await(await mcp(observerKey,'tools/call',{name:'reports_save',arguments:{}})).json()).error);
 assert.ok((await api(v,{op:'telegramStatus'})).error);
 await api(a,{op:'telegramObserverRevoke'});assert.equal((await mcp(observerKey,'tools/list')).status(),401);

 const metric=await sdkTool('engine_read',{id:'value'});assert.equal(metric.sample.value.version,1);
 const subscription=await sdkTool('engine_subscribe',{ids:['value']});
 assert.equal((await sdkTool('engine_poll',{subscriptionId:subscription.subscriptionId})).subscriptionId,subscription.subscriptionId);
 // A second key for this very same account cannot borrow the first key's lease.
 const otherKey=await api(a,{op:'agentKeyCreate'});
 const borrowed=await(await mcp(otherKey.key,'tools/call',{name:'engine_poll',arguments:{subscriptionId:subscription.subscriptionId}})).json();
 assert.equal(borrowed.result.isError,true);assert.equal(borrowed.result.structuredContent.error,'not_found');
 await api(a,{op:'agentKeyRevoke',id:otherKey.id});
 await sdk.close();
 // Reconnecting with the original key retains the key-scoped subscription.
 await sdk.connect(new StreamableHTTPClientTransport(new URL(origin+'/mcp'),{requestInit:{headers:{Authorization:'Bearer '+agentKey}},fetch:fixtureFetch}));
 assert.equal((await sdkTool('engine_poll',{subscriptionId:subscription.subscriptionId})).subscriptionId,subscription.subscriptionId);
 await sdkTool('engine_unsubscribe',{subscriptionId:subscription.subscriptionId});
 await a.locator('.lv-sidebar a[href="#dashboard"]').click();
 await a.waitForFunction(()=>window.dashboardReport?.registration?.connected);
 const liveTarget=await a.evaluate(()=>{const r=talia.registration();return {clientId:r.clientId,slotId:r.slotId,liveInstanceId:r.liveInstanceId};});
 const listed=await sdkTool('clients_list');
 const liveSlot=listed.clients.find(c=>c.clientId===liveTarget.clientId).slots.find(s=>s.liveInstanceId===liveTarget.liveInstanceId);
 assert.equal((await sdkTool('live_inspect',{target:liveTarget})).snapshot.dirty,false);
 const edit=await sdkTool('live_execute',{target:liveTarget,expectedEditRevision:0,source:'const s=ctx.state();await ctx.commit(s,{...s.value,title:{text:"HTTP MCP edit"}});',requestId:crypto.randomUUID()});
 assert.equal(edit.audit.status,'complete');await a.waitForFunction(()=>dashboardReport.dirty&&dashboardReport.state.title.text==='HTTP MCP edit');
 const reloadArgs={target:liveTarget,expectedEditRevision:1,expectedAssignmentRevision:liveSlot.desiredAssignment.revision,requestId:crypto.randomUUID()};
 const deniedReload=await sdk.callTool({name:'live_reload',arguments:reloadArgs});assert.equal(deniedReload.isError,true);assert.equal(deniedReload.structuredContent.error,'dirty_ack_required');
 const reloadId=crypto.randomUUID();
 const reloaded=await sdkTool('live_reload',{...reloadArgs,discardDirty:true,requestId:reloadId});assert.equal(reloaded.audit.status,'complete');
 await a.waitForFunction(old=>dashboardReport.registration.connected&&dashboardReport.registration.liveInstanceId!==old&&!dashboardReport.dirty,liveTarget.liveInstanceId);
 assert.equal((await sdkTool('operation_status',{requestId:reloadId})).outcome.liveInstanceId,await a.evaluate(()=>dashboardReport.registration.liveInstanceId));
 await sdk.close();
  const toolList=await(await mcp(agentKey,'tools/list')).json();assert.ok(toolList.result.tools.some(t=>t.name==='dashboard_access_list'));
 const policyResult=await(await mcp(agentKey,'tools/call',{name:'dashboard_access_list',arguments:{}})).json();assert.equal(policyResult.result.isError,false);
 assert.equal((await mcp(agentKey,'tools/list',{}, {Origin:'https://foreign.invalid'})).status(),403);
 assert.equal((await admin.request.get(origin+'/mcp',{headers:{Authorization:'Bearer '+agentKey,Origin:'https://foreign.invalid',Accept:'text/event-stream'}})).status(),403);
 assert.equal((await mcp(agentKey,'tools/list',{}, {'MCP-Protocol-Version':'invalid'})).status(),400);
 const viewerKey=await api(v,{op:'agentKeyCreate'});assert.equal((await(await mcp(viewerKey.key,'tools/list')).json()).result.tools.length,0);
 assert.ok((await(await mcp(viewerKey.key,'tools/call',{name:'dashboard_access_list',arguments:{}})).json()).error);
 await api(v,{op:'agentKeyRevoke',id:keyId});assert.equal((await mcp(agentKey,'tools/list')).status(),200);
 await a.locator('.lv-sidebar a[href="#dashboard"]').click();await a.waitForFunction(()=>document.querySelector('#agent-key-value').value==='');
 assert.equal(await a.evaluate(k=>JSON.stringify({...localStorage,...sessionStorage}).includes(k),agentKey),false);
  assert.equal((await api(v,{op:'role',subject:issuer+'#viewer',admin:true})).error,'forbidden');
 await a.waitForFunction(()=>window.dashboardReport?.registration?.connected);await a.locator('.lv-sidebar a[href="#sharing"]').click();// Shared components must retain the live runtime and unsaved sharing form.
 const liveId=await a.evaluate(()=>window.dashboardReport.registration.liveInstanceId);
 await a.locator('#share-public').check();
 await a.getByRole('button',{name:'Change theme: System',exact:true}).click();await a.getByRole('button',{name:'Dark',exact:true}).click();
 assert.equal(await a.locator('.lello-theme').getAttribute('data-lello-theme'),'blue-dark');
 await a.getByRole('button',{name:'Collapse sidebar',exact:true}).click();
 await a.waitForFunction(()=>Math.round(document.querySelector('.lv-sidebar').getBoundingClientRect().width)===80);
 await a.locator('.lv-sidebar a[href="#settings"]').click();await a.locator('.lv-sidebar a[href="#sharing"]').click();
 await a.waitForTimeout(10500);assert.equal(refreshCounts.get('admin'),1);assert.equal(refreshCounts.get('viewer'),1);assert.ok((await admin.cookies()).find(c=>c.name==='__Host-talia-session').expires-Date.now()/1000>29*86400);assert.equal(await a.locator('#share-public').isChecked(),true);
 assert.equal(await a.evaluate(()=>window.dashboardReport.registration.liveInstanceId),liveId);
 await a.emulateMedia({reducedMotion:'reduce'});assert.equal(await a.locator('.lv-scaffold').evaluate(e=>getComputedStyle(e).transitionDuration),'0s');await a.getByRole('button',{name:'Expand sidebar',exact:true}).click();
 await a.getByRole('button',{name:'Change theme: Dark',exact:true}).click();await a.keyboard.press('Home');await a.keyboard.press('Enter');
 assert.equal(await a.locator('.lello-theme').getAttribute('data-lello-theme'),'blue-light');
 assert.equal(await a.getByRole('button',{name:'Change theme: Light',exact:true}).evaluate(e=>e===document.activeElement),true);
 await a.locator('#share-public').check();await a.locator('#save-sharing').click();await a.getByText('Dashboard sharing saved.',{exact:true}).waitFor();
 await v.locator('#refresh-dashboards').click();await v.waitForFunction(()=>window.dashboardReport?.registration?.connected&&window.dashboardReport.state.history.length>0);assert.equal(await v.evaluate(()=>window.taliaViewer),true);
 await v.locator('#make-default').click();await v.getByText('New clients will open this dashboard automatically.',{exact:true}).waitFor();
 const catalog=await api(v,{op:'catalog'});assert.equal(catalog.defaultDashboard,'monitor');assert.deepEqual(catalog.dashboards,[{id:'monitor'}]);
 const engineCall=(p,op,args)=>p.evaluate(async({op,args})=>(await(await fetch('/engine',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({version:1,epoch:1,client:'probe',op,args,dashboard:window.taliaDashboard})})).json()),{op,args});
 assert.equal((await engineCall(v,'write',{id:'value'})).error,'forbidden');assert.equal((await engineCall(v,'monitoringConfig',{})).error,'forbidden');
 const another=await browser.newContext({ignoreHTTPSErrors:true});const v2=await login(another);await v2.waitForFunction(()=>window.dashboardReport?.registration?.connected);assert.equal(await v2.evaluate(()=>window.taliaDashboard.id),'monitor');
 await v.setViewportSize({width:390,height:844});assert.equal(await v.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);mkdirSync('.local/access-verification',{recursive:true});await v.getByRole('button',{name:'Open navigation',exact:true}).click();assert.equal(await v.locator('.lv-drawer .lv-account').count(),1);assert.equal(await v.locator('.lv-header .lv-account').count(),0);await v.keyboard.press('Escape');await v.screenshot({path:'.local/access-verification/viewer-mobile.png',fullPage:true});await a.locator('.lv-sidebar a[href="#dashboard"]').click();await a.locator('[data-shell-page=dashboard]').waitFor({state:'visible'});await a.screenshot({path:'.local/access-verification/admin-desktop.png',fullPage:true});
 // Entering/exiting fullscreen retains the live dashboard instance and uses the full viewport.
 const beforeDisplay=await a.evaluate(()=>window.dashboardReport.registration.liveInstanceId);
 await a.locator('#monitoring-enter').click();await a.waitForFunction(()=>document.fullscreenElement?.id==='monitoring-surface');
 assert.equal(await a.locator('#monitoring-surface').evaluate(e=>e.clientWidth),await a.evaluate(()=>innerWidth));
 assert.equal(await a.evaluate(()=>window.dashboardReport.registration.liveInstanceId),beforeDisplay);
 await a.locator('#monitoring-exit').click();await a.waitForFunction(()=>!document.fullscreenElement);
 assert.equal(await a.evaluate(()=>window.dashboardReport.registration.liveInstanceId),beforeDisplay);
 // Monitoring selections refuse dirty live edits before changing the server assignment.
 await a.evaluate(()=>window.talia.live("void 0;"));
 const dirtySelection=await a.evaluate(async()=>{try{await window.talia.selectDashboard(window.taliaDashboard.id,{}, {},true);return 'unexpected success';}catch(e){return e.message;}});
 assert.match(dirtySelection,/Temporary dashboard edits/);
 assert.equal(await a.evaluate(()=>window.dashboardReport.registration.liveInstanceId),beforeDisplay);
 await a.evaluate(()=>window.talia.reload());
 // A live edit arriving while a new package is fetched must also survive.
 const beforeLateEdit=await a.evaluate(()=>window.dashboardReport.registration.liveInstanceId);
 let releasePackage,packageArrived;const packageHeld=new Promise(r=>releasePackage=r),packageWaiting=new Promise(r=>packageArrived=r);
 await a.route('**/dashboard/shared/vm.js',async route=>{const response=await route.fetch();packageArrived();await packageHeld;await route.fulfill({response});});
 await a.evaluate(()=>{window.pendingMonitoringSelection=window.talia.selectDashboard(window.taliaDashboard.id,{}, {},true).then(()=>null,e=>e.message);});
 await packageWaiting;await a.evaluate(()=>window.talia.live('void 0;'));releasePackage();
 assert.match(await a.evaluate(()=>window.pendingMonitoringSelection),/Temporary dashboard edits/);
 assert.equal(await a.evaluate(()=>window.dashboardReport.registration.liveInstanceId),beforeLateEdit);
 await a.unroute('**/dashboard/shared/vm.js');await a.evaluate(()=>window.talia.reload());
 // System appearance follows OS changes without replacing the runtime.
 await v.getByRole('button',{name:'Change theme: System',exact:true}).click();await v.getByRole('button',{name:'Dark',exact:true}).click();
 await v.reload();await v.waitForFunction(()=>window.dashboardReport?.registration?.connected);assert.equal(await v.locator('.lello-theme').getAttribute('data-lello-theme'),'blue-dark');
 await v.getByRole('button',{name:'Change theme: Dark',exact:true}).click();await v.getByRole('button',{name:'System',exact:true}).click();await v.emulateMedia({colorScheme:'dark'});await v.waitForFunction(()=>document.querySelector('.lello-theme').dataset.lelloTheme==='blue-dark');
 await v.emulateMedia({colorScheme:'light'});await v.waitForFunction(()=>document.querySelector('.lello-theme').dataset.lelloTheme==='blue-light');
 await v.getByRole('button',{name:'Open navigation',exact:true}).click();await v.setViewportSize({width:1280,height:900});await v.waitForFunction(()=>!document.querySelector('.lv-drawer').open);assert.equal(await v.locator('.lv-sidebar .lv-account').count(),1);
 await v.setViewportSize({width:320,height:760});assert.equal(await v.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
 // Session and account selection survive page reload past the original ID/access lifetime.
 await v.reload();await v.waitForFunction(()=>window.dashboardReport?.registration?.connected);assert.equal(await v.evaluate(()=>window.taliaDashboard.id),'monitor');
 await a.locator('.lv-sidebar a[href="#sharing"]').click();await a.locator('#share-public').uncheck();await a.locator('#save-sharing').click();await a.getByText('Dashboard sharing saved.',{exact:true}).waitFor();await v.getByRole('heading',{name:'No dashboards available yet'}).waitFor({timeout:15000});
 assert.equal((await engineCall(v,'hello',{})).error,'forbidden');assert.equal((await api(v,{op:'catalog'})).defaultDashboard,null);
 // A create response arriving after navigation cannot reveal or strand a key.
 await a.locator('.lv-sidebar a[href="#settings"]').click();
 let releaseCreate,creationArrived;const held=new Promise(r=>releaseCreate=r),arrived=new Promise(r=>creationArrived=r);
 await a.route('**/account',async route=>{if(route.request().postDataJSON()?.op==='agentKeyCreate'){const response=await route.fetch();creationArrived();await held;await route.fulfill({response});}else await route.continue();});
 await a.locator('#agent-key-create').click();await arrived;await a.locator('.lv-sidebar a[href="#dashboard"]').click();releaseCreate();
 await a.waitForFunction(()=>!document.querySelector('#agent-key-create').disabled);assert.equal(await a.locator('#agent-key-value').inputValue(),'');await a.unroute('**/account');assert.equal((await api(a,{op:'agentKeys'})).keys.length,1);
  // Revocation, expiry and logout apply to fresh calls using the same key.
 await a.locator('.lv-sidebar a[href="#settings"]').click();await a.locator('#agent-key-list button').first().click();await a.getByText('Agent key revoked.',{exact:true}).waitFor();assert.equal((await mcp(agentKey,'tools/list')).status(),401);
 const expiring=await api(a,{op:'agentKeyCreate'});
 execFileSync('python3',['-c',"import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('UPDATE user_agent_keys SET expires=0 WHERE id=?',(sys.argv[2],)); c.commit()",tmp+'/engine.db',expiring.id]);
 assert.equal((await mcp(expiring.key,'tools/list')).status(),401);
 const logoutKey=await api(a,{op:'agentKeyCreate'});await a.evaluate(()=>fetch('/auth/logout',{method:'POST'}));assert.equal((await mcp(logoutKey.key,'tools/list')).status(),401);
  console.log(JSON.stringify({passed:true,checks:['web Telegram settings and read-only observer HTTP MCP credential, write denial and revocation','server-side report authoring, preview execution and retained HTML over HTTP MCP','fullscreen retains live dashboard and monitoring selection preserves dirty edits','browser Web Push enrollment, disable, endpoint rejection, viewer isolation and real worker authenticated detail fetch','HTTP MCP engine read and key-scoped subscriptions survive reconnect','HTTP MCP live inspect/execute/reload and dirty guards','browser remains signed in past initial token expiry with rotated refresh token','persistent 30-day HttpOnly session cookie','official SDK HTTP MCP authoring with user-attributed audit','HTTP MCP initialize/list/call','one-time key UI and clipboard configuration','navigation during key creation revokes the unseen key','expiry/revocation/logout enforcement','viewer MCP isolation','Origin/protocol validation','shared LelloDesign responsive shell','theme and sidebar preserve live runtime','polling preserves sharing drafts','keyboard theme focus','mobile account placement','real OIDC boundary with local provider','viewer starts empty','admin shares from shell','viewer loads public dashboard','server rejects writes and config','account default opens on new installation','reload retains selection','mobile layout fits viewport','unsharing blocks active viewer']}));
}finally{await browser?.close();if(engine?.pid){try{process.kill(-engine.pid,'SIGTERM');}catch{}}proxy?.close();provider?.close();rmSync(tmp,{recursive:true,force:true});}
