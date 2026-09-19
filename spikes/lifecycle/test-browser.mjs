import {chromium} from '../runtime/node_modules/playwright/index.mjs';
import http from 'node:http';import {readFile,mkdtemp,rm} from 'node:fs/promises';import os from 'node:os';import path from 'node:path';import {spawn} from 'node:child_process';
const port=Number(process.argv[2]),root=path.resolve('spikes'),checks=[],run=Date.now();
const xvfb=spawn('Xvfb',['-displayfd','1','-screen','0','1280x800x24']);
const display=await new Promise((resolve,reject)=>{xvfb.stdout.once('data',d=>resolve(':'+d.toString().trim()));xvfb.once('error',reject);});
const server=http.createServer(async(req,res)=>{try{
 if(req.url==='/rpc'){const chunks=[];for await(const c of req)chunks.push(c);const r=await fetch(`http://127.0.0.1:${port}/rpc`,{method:'POST',headers:{'content-type':'application/json'},body:Buffer.concat(chunks)});res.writeHead(r.status,{'content-type':'application/json'});res.end(await r.text());return;}
 let file=req.url==='/'?'/lifecycle/index.html':req.url;
 if(file.startsWith('/web/')||file.startsWith('/shared/'))file='/runtime'+file;
 file=path.resolve(root,'.'+file);if(!file.startsWith(root+path.sep))throw Error('path');
 res.setHeader('content-type',file.endsWith('.js')?'application/javascript':file.endsWith('.wasm')?'application/wasm':'text/html');res.end(await readFile(file));
 }catch(e){res.writeHead(404);res.end(String(e));}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));let browser;let seq=0;
async function rpc(op,args){const r=await fetch(`http://127.0.0.1:${port}/rpc`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({session:'lifecycle-browser',channel:'test',epoch:1,id:++seq,op,args})});const body=await r.json();if(body.error)throw Error(body.error);return body.value;}
const check=(ok,n)=>{if(!ok)throw Error(n);checks.push(n);};
let chrome,ws,userData;
try{
 userData=await mkdtemp(path.join(os.tmpdir(),'talia-chrome-'));
 chrome=spawn(chromium.executablePath(),['--no-sandbox','--no-first-run','--remote-debugging-port=0','--user-data-dir='+userData,'about:blank'],{env:{...process.env,DISPLAY:display},stdio:['ignore','ignore','pipe']});
 const endpoint=await new Promise((resolve,reject)=>{let text='';chrome.stderr.on('data',d=>{text+=d;const m=text.match(/DevTools listening on (ws:\/\/[^\s]+)/);if(m)resolve(m[1]);});chrome.once('error',reject);});
 ws=new WebSocket(endpoint);await new Promise(r=>ws.addEventListener('open',r,{once:true}));let id=0;const waiting=new Map();
 ws.addEventListener('message',event=>{const data=JSON.parse(event.data),p=waiting.get(data.id);if(p){waiting.delete(data.id);data.error?p.reject(Error(JSON.stringify(data.error))):p.resolve(data.result);}});
 const send=(method,params={},sessionId)=>new Promise((resolve,reject)=>{const n=++id;waiting.set(n,{resolve,reject});ws.send(JSON.stringify({id:n,method,params,...(sessionId?{sessionId}:{})}));});
 const context={async newPage(){const {targetId}=await send('Target.createTarget',{url:'about:blank'});const {sessionId}=await send('Target.attachToTarget',{targetId,flatten:true});
  const page={async evaluate(fn,arg){const r=await send('Runtime.evaluate',{expression:`(${fn.toString()})(${JSON.stringify(arg)??''})`,returnByValue:true,awaitPromise:true,userGesture:true},sessionId);if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value;},
   async waitForFunction(fn,arg){for(let i=0;i<150;i++){try{if(await page.evaluate(fn,arg))return;}catch{}await new Promise(r=>setTimeout(r,100));}throw Error('condition timeout: '+await page.evaluate(()=>JSON.stringify({hidden:document.hidden,report:window.lifecycleReport})));},
   async goto(url){await send('Page.navigate',{url},sessionId);},async click(selector){await page.evaluate(s=>document.querySelector(s).click(),selector);},async bringToFront(){await send('Target.activateTarget',{targetId});},async reload(){await send('Page.reload',{},sessionId);}};return page;}};
 browser={async close(){await send('Browser.close');ws.close();},version:async()=>(await send('Browser.getVersion')).product};
 const page=await context.newPage();await page.goto(`http://127.0.0.1:${server.address().port}`);await page.waitForFunction(()=>window.lifecycleReport?.snapshot);
 await page.click('#dirty');await page.waitForFunction(()=>window.lifecycleReport.local.dirty);
 const action=await page.evaluate(()=>window.nextActionId());await rpc('test',{command:'hold',key:action});await page.click('#action');
 await page.waitForFunction(id=>window.lifecycleReport.outcomes[id],action);
 const other=await context.newPage();await other.goto('about:blank');await other.bringToFront();await page.waitForFunction(()=>document.hidden && !window.lifecycleReport.active);
 const before=await page.evaluate(()=>window.lifecycleReport.local.ticks);await rpc('test',{command:'release',key:action});await new Promise(r=>setTimeout(r,400));
 check((await rpc('status',{actionId:action})).status==='completed','server action completes while hidden');
 check(await page.evaluate(t=>window.lifecycleReport.local.ticks===t && window.lifecycleReport.subscriptions===0,before),'local execution and subscription paused');
 await page.bringToFront();await page.waitForFunction(id=>window.lifecycleReport.snapshot.value===42 && window.lifecycleReport.outcomes[id]?.status==='completed',action);
 check(await page.evaluate(()=>window.lifecycleReport.local.dirty && Object.keys(window.lifecycleReport.outcomes).length===1),'resume retains dirty state and reconciles without resubmission');
 for(let n=0;n<3;n++){await other.bringToFront();await page.waitForFunction(()=>!window.lifecycleReport.active,null,{polling:100});await rpc('action',{actionId:'external-'+run+'-'+n,value:60+n});await page.bringToFront();await page.waitForFunction(v=>window.lifecycleReport.snapshot.value===v && window.lifecycleReport.subscriptions===1,60+n);}
 check(true,'repeated actual visibility transitions');
 await page.reload();await page.waitForFunction(()=>window.lifecycleReport?.snapshot?.value===62 && !window.lifecycleReport.local.dirty);
 check(true,'reload restores saved baseline and preserves server effect');
 console.log(JSON.stringify({passed:true,checks,browser:await browser.version(),final:await page.evaluate(()=>window.lifecycleReport)}));
}finally{await browser?.close();chrome?.kill();server.close();xvfb.kill();if(userData)await rm(userData,{recursive:true,force:true});}
