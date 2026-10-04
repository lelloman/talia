import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {createServer} from 'node:http';
import {readFile,mkdir} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import assert from 'node:assert/strict';
const root=resolve('.');
const server=createServer(async(req,res)=>{try{
 const path=resolve(root,'.'+new URL(req.url,'http://localhost').pathname);
 if(!path.startsWith(root+'/'))throw Error('path');
 res.setHeader('Content-Type',({'.js':'text/javascript','.html':'text/html','.css':'text/css','.svg':'image/svg+xml','.woff2':'font/woff2'})[extname(path)]||'application/octet-stream');
 res.end(await readFile(path));
}catch{res.statusCode=404;res.end();}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
await mkdir('.local/chats',{recursive:true});
// In-memory stand-in for the engine chat API (see engine/src/chat.rs).
const fake=`
 import {mountChrome} from './dist/chrome.js';
 const shell=await mountChrome({name:'Chat test',subject:'fixture'});shell.update({isAdmin:true,connected:true});
 const sessions=[],requests=[],byClient=new Map();let next=1;
 window.calls=[];window.lose=false;window.forbid=false;
 window.finish=(answer='The root filesystem is 41% free; /mnt/data is at 6% and needs attention.')=>{for(const r of requests)if(r.status==='running'){r.status='done';r.answer=answer;r.steps=[];}};
 const admit=(session,{requestId,text})=>{const key=session+'/'+requestId;if(byClient.has(key))return byClient.get(key);
  const r={id:next++,session,requestId,status:'running',text,answer:null,error:null,report:null,created:Date.now(),
   steps:[{label:'Checked the monitoring overview',done:true},{label:'Read host-homelab history',done:false}]};
  requests.push(r);byClient.set(key,r.id);const s=sessions.find(s=>s.id===session);s.updated=Date.now();return r.id;};
 window.taliaReportSchedules(async ({operation,args})=>{
  if(operation==='list')return {definitions:[{id:'infra',enabled:true,scheduled:false,available:true,latest:{id:'infra-20261004-0900',status:'complete',created:Date.now()}}]};
  if(operation==='schedule_get')return {id:'infra',version:1,enabled:false,schedule:null,next_due:null,destinations:[]};
 });
 window.taliaChats(async body=>{
  window.calls.push(body);
  if(window.forbid){const e=Error('forbidden');e.serverRejected=true;throw e;}
  const {op,...a}=body;let result;
  switch(op){
   case 'chatList':result={sessions:sessions.filter(s=>!s.deleted).map(s=>({...s,running:requests.some(r=>r.session===s.id&&r.status==='running')})).sort((x,y)=>y.updated-x.updated)};break;
   case 'chatCreate':{let s=sessions.find(s=>s.client===a.requestId);if(!s){s={id:'chat-'+next++,client:a.requestId,title:a.text.split('\\n')[0].slice(0,120),created:Date.now(),updated:Date.now()};sessions.push(s);}
    result={session:s.id,request:admit(s.id,a)};break;}
   case 'chatSend':result={session:a.session,request:admit(a.session,a)};break;
   case 'chatGet':{const s=sessions.find(s=>s.id===a.session&&!s.deleted);if(!s){const e=Error('not_found');e.serverRejected=true;throw e;}
    result={session:s,requests:requests.filter(r=>r.session===s.id).map(r=>structuredClone(r))};break;}
   case 'chatStop':for(const r of requests)if(r.session===a.session&&r.status==='running')r.status='stopped';result={stopped:1};break;
   case 'chatRename':sessions.find(s=>s.id===a.session).title=a.title;result={};break;
   case 'chatDelete':sessions.find(s=>s.id===a.session).deleted=true;result={deleted:true};break;
  }
  if(window.lose&&(op==='chatSend'||op==='chatCreate')){window.lose=false;throw Error('network response lost');}
  return structuredClone(result);
 });`;
const browser=await chromium.launch({headless:true});
try{
 const page=await browser.newPage({viewport:{width:1280,height:860}});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/auth.js',route=>route.fulfill({contentType:'text/javascript',body:fake}));
 await page.goto(`http://127.0.0.1:${server.address().port}/dashboard/web/index.html#chats`);
 await page.getByRole('heading',{name:'Ask Talìa about your homelab'}).waitFor();
 const box=page.getByLabel('Message');
 await box.fill('How is the disk on homelab?');await box.press('Enter');
 await page.getByText('Investigating…').waitFor();
 await page.getByText('Read host-homelab history').waitFor();
 assert.equal(await page.getByRole('button',{name:'Stop'}).count(),1);
 await page.screenshot({path:'.local/chats/running-1280.png'});
 await page.evaluate(()=>window.finish());
 await page.getByText('The root filesystem is 41% free',{exact:false}).waitFor({timeout:5000});
 assert.equal(await box.inputValue(),'');
 // A lost response keeps the message and its request ID; Retry cannot duplicate it.
 await page.evaluate(()=>window.lose=true);
 await box.fill('And /mnt/data?');await box.press('Enter');
 await page.getByText('Message not confirmed',{exact:false}).waitFor();
 assert.equal(await box.inputValue(),'And /mnt/data?');
 await page.getByRole('button',{name:'Retry'}).click();
 await page.getByText('Investigating…').waitFor();
 const sends=await page.evaluate(()=>window.calls.filter(c=>c.op==='chatSend'));
 assert.equal(sends.length,2);assert.equal(sends[0].requestId,sends[1].requestId);
 assert.equal(await page.locator('.talia-chat-turn').count(),2);
 await page.evaluate(()=>window.finish('Free space on /mnt/data is 6.2% (248 GB of 4 TB).'));
 await page.getByText('Free space on /mnt/data is 6.2%',{exact:false}).waitFor({timeout:5000});
 await page.screenshot({path:'.local/chats/conversation-1280.png'});
 // A second session runs independently; stopping it leaves the first untouched.
 await page.getByRole('button',{name:'New chat'}).click();
 await box.fill('Any alerts firing right now?');await box.press('Enter');
 await page.getByText('Investigating…').waitFor();
 await page.getByRole('button',{name:'Stop'}).click();
 await page.getByText('Stopped before an answer was ready.').waitFor();
 assert.equal(await page.locator('.talia-chat-item').count(),2);
 await page.getByRole('button',{name:'Rename'}).click();
 await page.getByLabel('Chat title').fill('Alert check');
 await page.getByRole('button',{name:'Save'}).click();
 await page.getByRole('heading',{name:'Alert check'}).waitFor();
 await page.getByRole('button',{name:'Delete'}).click();
 await page.getByRole('button',{name:'Delete chat'}).click();
 await page.waitForFunction(()=>document.querySelectorAll('.talia-chat-item').length===1);
 await page.locator('.talia-chat-item').first().click();
 await page.getByText('Free space on /mnt/data is 6.2%',{exact:false}).waitFor();
 await page.getByRole('button',{name:/Change theme/}).click();await page.getByRole('button',{name:'Dark',exact:true}).click();
 await page.waitForTimeout(300);await page.screenshot({path:'.local/chats/conversation-dark-1280.png'});
 await page.setViewportSize({width:390,height:800});
 await page.waitForTimeout(200);await page.screenshot({path:'.local/chats/conversation-dark-390.png'});
 await page.getByRole('button',{name:'‹ Chats'}).click();
 await page.getByRole('button',{name:'New chat'}).waitFor();
 await page.screenshot({path:'.local/chats/list-dark-390.png'});
 assert.equal(await page.evaluate(()=>document.querySelector('.lv-main').scrollWidth<=document.querySelector('.lv-main').clientWidth),true);
 // Investigate a report run: a prefilled draft with the run attached only to the new session.
 await page.setViewportSize({width:1280,height:860});
 await page.evaluate(()=>location.hash='#reports');
 await page.getByRole('button',{name:'infra',exact:true}).click();
 await page.getByRole('button',{name:'Investigate latest run'}).click();
 await page.getByText('Report run infra-20261004-0900').waitFor();
 assert.match(await box.inputValue(),/Investigate report run infra-20261004-0900/);
 await page.getByRole('button',{name:'Cancel'}).click();
 await page.waitForFunction(()=>location.hash==='#reports');
 await page.getByRole('button',{name:'Investigate latest run'}).click();
 await page.evaluate(()=>location.hash='#reports');
 await page.getByRole('button',{name:'Investigate latest run'}).click();
 await box.fill('Why did infra flag /mnt/data?');await box.press('Enter');
 await page.getByText('Investigating…').waitFor();
 await page.evaluate(()=>window.finish('The data volume crossed the 10% free threshold overnight.'));
 await page.getByText('crossed the 10% free threshold',{exact:false}).waitFor({timeout:5000});
 await box.fill('Is it still shrinking?');await box.press('Enter');
 await page.getByText('Investigating…').waitFor();
 const investigation=await page.evaluate(()=>window.calls.filter(c=>c.op==='chatCreate'||c.op==='chatSend').slice(-2));
 assert.equal(investigation[0].op,'chatCreate');assert.equal(investigation[0].report,'infra-20261004-0900');
 assert.equal(investigation[1].op,'chatSend');assert.equal(investigation[1].report,undefined);
 assert.equal(await page.evaluate(()=>window.calls.filter(c=>c.op==='chatCreate'&&c.report).length),1);
 await page.screenshot({path:'.local/chats/investigate-1280.png'});
 // Viewers (or revoked admins) see a clear forbidden state.
 await page.evaluate(()=>{window.forbid=true;location.hash='#dashboard';});
 await page.evaluate(()=>location.hash='#chats');
 await page.getByText('Chat requires administrator access.').waitFor();
 assert.deepEqual(errors,[]);
 console.log('PASS: chat send/steps/answer, report investigation (single create, attachment only on the new session, cancel), lost-response retry without duplicates, parallel sessions, stop, rename, delete, forbidden, light/dark/390');
}finally{await browser.close();server.close();}
