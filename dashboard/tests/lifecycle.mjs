// Actual Chromium tab visibility, without Playwright's background-throttling flags.
import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {spawn} from 'node:child_process';import {once} from 'node:events';import {mkdtemp,rm} from 'node:fs/promises';import os from 'node:os';import path from 'node:path';import assert from 'node:assert/strict';
const server=spawn('python3',['dashboard/serve.py','0'],{stdio:['ignore','pipe','inherit']});const {port}=JSON.parse(String((await once(server.stdout,'data'))[0]));
const xvfb=spawn('Xvfb',['-displayfd','1','-screen','0','1280x900x24']);const display=':'+String((await once(xvfb.stdout,'data'))[0]).trim();
let chrome,ws,userData;const checks=[];let sequence=0;
async function rpc(op,args){const r=await fetch(`http://127.0.0.1:${port}/rpc`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({session:'p1-dashboard',channel:'test',epoch:1,id:++sequence,op,args})});const reply=await r.json();if(reply.error)throw Error(reply.error);return reply.value;}
try{
 userData=await mkdtemp(path.join(os.tmpdir(),'talia-p1-chrome-'));
 chrome=spawn(chromium.executablePath(),['--no-sandbox','--no-first-run','--remote-debugging-port=0','--user-data-dir='+userData,'about:blank'],{env:{...process.env,DISPLAY:display},stdio:['ignore','ignore','pipe']});
 const endpoint=await new Promise((resolve,reject)=>{let text='';chrome.stderr.on('data',d=>{text+=d;const m=text.match(/DevTools listening on (ws:\/\/[^\s]+)/);if(m)resolve(m[1]);});chrome.once('error',reject);});
 ws=new WebSocket(endpoint);await once(ws,'open');let id=0;const pending=new Map();ws.addEventListener('message',event=>{const r=JSON.parse(event.data),p=pending.get(r.id);if(p){pending.delete(r.id);r.error?p.reject(Error(JSON.stringify(r.error))):p.resolve(r.result);}});
 const send=(method,params={},sessionId)=>new Promise((resolve,reject)=>{const n=++id;pending.set(n,{resolve,reject});ws.send(JSON.stringify({id:n,method,params,...(sessionId?{sessionId}:{})}));});
 async function page(){const {targetId}=await send('Target.createTarget',{url:`http://127.0.0.1:${port}`});const {sessionId}=await send('Target.attachToTarget',{targetId,flatten:true});return {
  async evaluate(fn,arg){const r=await send('Runtime.evaluate',{expression:`(${fn.toString()})(${JSON.stringify(arg)??''})`,returnByValue:true,awaitPromise:true,userGesture:true},sessionId);if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value;},
  async front(){await send('Target.activateTarget',{targetId});}
 };}
 async function wait(p,fn,arg){for(let i=0;i<150;i++){try{if(await p.evaluate(fn,arg))return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('condition timeout: '+JSON.stringify(await p.evaluate(()=>({hidden:document.hidden,report:window.dashboardReport,diagnostic:document.querySelector('#diagnostic').textContent}))));}
 const p=await page();await wait(p,()=>window.dashboardReport?.state.history.length>0);
 await p.evaluate(()=>talia.live("TaliaVM.replaceAction('controls',c=>{const s=c.state();c.commit(s,{...s.value,screen:'controlsScreen'});});"));await wait(p,()=>dashboardReport.dirty);
 await p.evaluate(()=>[...document.querySelectorAll('button')].find(x=>x.textContent==='Controls').click());await wait(p,()=>dashboardReport.state.screen==='controlsScreen');
 await p.evaluate(()=>[...document.querySelectorAll('button')].find(x=>x.textContent==='Apply value').click());await wait(p,()=>dashboardReport.actions[0]?.status==='completed');
 const action=await p.evaluate(()=>dashboardReport.nextActionId);await rpc('test',{command:'hold',key:action});
 await p.evaluate(()=>[...document.querySelectorAll('button')].find(x=>x.textContent==='Controls').click());await wait(p,()=>dashboardReport.state.screen==='controlsScreen');
 await p.evaluate(()=>[...document.querySelectorAll('button')].find(x=>x.textContent==='Apply value').click());await wait(p,id=>dashboardReport.actionIds.includes(id),action);
 const other=await page();await other.front();await wait(other,()=>window.dashboardReport?.state.history.length>0);await wait(p,()=>document.hidden&&dashboardReport.paused&&dashboardReport.subscriptions===0);
 const before=await p.evaluate(()=>JSON.stringify(dashboardReport.state));await rpc('test',{command:'release',key:action});
 for(let i=0;i<50;i++){if((await rpc('status',{actionId:action})).status==='completed')break;await new Promise(r=>setTimeout(r,100));}
 assert.equal((await rpc('status',{actionId:action})).status,'completed');assert.equal(await p.evaluate(()=>JSON.stringify(dashboardReport.state)),before);checks.push('actual hidden tab pauses local state/subscriptions while server effect completes');
 await p.front();await wait(p,()=>dashboardReport.state.history.includes(25)&&dashboardReport.actions.length===2&&dashboardReport.actions.every(a=>a.status==='completed')&&!dashboardReport.state.busy);
 assert.equal(await p.evaluate(()=>dashboardReport.actionIds.length),2);assert.equal(await p.evaluate(()=>dashboardReport.dirty),true);checks.push('resume retains dirty VM, reconciles outcome and refreshes without resubmission');
 for(let n=0;n<3;n++){await other.front();await wait(p,()=>document.hidden&&dashboardReport.paused);await rpc('action',{actionId:'external-'+n,value:60+n});await p.front();await wait(p,v=>dashboardReport.state.history.at(-1)===v&&dashboardReport.subscriptions===1,60+n);}checks.push('repeated visibility transitions do not leak subscriptions');
 for(const source of ["throw Error('test internal failure')",'while(true){}','globalThis.buffers=[];for(let i=0;i<100;i++)buffers.push(new ArrayBuffer(1024*1024));']){
  await p.evaluate(s=>talia.live(s),source);await wait(p,()=>dashboardReport.failure&&dashboardReport.subscriptions===0&&!document.querySelector('#restart').hidden);
  await other.front();await wait(other,()=>!dashboardReport.failure&&dashboardReport.state.history.at(-1)===62);await p.front();assert.ok(await p.evaluate(()=>dashboardReport.failure));
  await p.evaluate(()=>document.querySelector('#restart').click());await wait(p,()=>!dashboardReport.failure&&!dashboardReport.dirty&&dashboardReport.state.history.at(-1)===62);
 }
 checks.push('exception, CPU and memory faults stop only affected dashboard; manual saved-baseline restart preserves server effects');
 console.log(JSON.stringify({passed:true,checks,final:await p.evaluate(()=>dashboardReport)}));await send('Browser.close');
}finally{ws?.close();if(chrome&&chrome.exitCode===null&&chrome.signalCode===null){const exited=once(chrome,'exit');chrome.kill();await exited;}xvfb.kill();server.kill('SIGTERM');await once(server,'exit');if(userData)await rm(userData,{recursive:true,force:true,maxRetries:5,retryDelay:200});}
