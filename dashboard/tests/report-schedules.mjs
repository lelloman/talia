import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import assert from 'node:assert/strict';
const root=resolve('.');
const server=createServer(async(req,res)=>{try{
 const path=resolve(root,'.'+new URL(req.url,'http://localhost').pathname);
 if(!path.startsWith(root+'/'))throw Error('path');
 res.setHeader('Content-Type',({'.js':'text/javascript','.html':'text/html','.css':'text/css','.svg':'image/svg+xml'})[extname(path)]||'application/octet-stream');
 res.end(await readFile(path));
}catch{res.statusCode=404;res.end();}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const browser=await chromium.launch({headless:true});
try{
 const page=await browser.newPage({viewport:{width:1200,height:900}});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/auth.js',route=>route.fulfill({contentType:'text/javascript',body:`
 import {mountChrome} from './dist/chrome.js';
 const shell=await mountChrome({name:'Schedule test',subject:'fixture'});shell.update({isAdmin:true,connected:true});
 window.fixture={id:'infra',version:1,enabled:true,schedule:{kind:'daily',time:'09:00',zone:'Europe/Rome',weekdays:[]},next_due:1790751600000,destinations:['telegram-home']};
 window.saves=[];window.lose=false;window.conflict=false;const requests=new Map();
 window.taliaReportSchedules(async ({operation,args})=>{
  args=JSON.parse(JSON.stringify(args));
  if(operation==='list')return {definitions:[window.fixture]};
  if(operation==='schedule_get')return structuredClone(window.fixture);
  if(operation==='schedule_save'){
   window.saves.push(args);
   if(requests.has(args.requestId))return requests.get(args.requestId);
   if(window.conflict){const e=Error('report version conflict; reload the schedule');e.serverRejected=true;throw e;}
   window.fixture={...window.fixture,...args,version:args.expected+1,next_due:args.enabled?1790751600000:null};
   requests.set(args.requestId,structuredClone(window.fixture));
   if(window.lose){window.lose=false;throw Error('network response lost');}
   return window.fixture;
  }
 });` }));
 await page.goto(`http://127.0.0.1:${server.address().port}/dashboard/web/index.html#reports`);
 await page.getByRole('button',{name:'infra',exact:true}).click();
 await page.getByRole('button',{name:'Edit schedule',exact:true}).click();
 await page.getByLabel('Time',{exact:true}).fill('10:30');
 await page.getByRole('button',{name:'Cancel',exact:true}).click();
 assert.equal(await page.evaluate(()=>saves.length),0);
 await page.getByRole('button',{name:'Edit schedule',exact:true}).click();
 assert.equal(await page.getByLabel('Time',{exact:true}).inputValue(),'09:00');
 await page.getByLabel('Monday',{exact:true}).check();
 await page.getByLabel('Time',{exact:true}).fill('10:30');
 await page.getByRole('button',{name:'Save schedule',exact:true}).click();
 await page.getByText('Schedule saved.',{exact:true}).waitFor();
 assert.deepEqual(await page.evaluate(()=>saves[0].schedule),{kind:'daily',time:'10:30',zone:'Europe/Rome',weekdays:[1]});
 await page.getByRole('button',{name:'Edit schedule',exact:true}).click();
 await page.getByLabel('Schedule enabled',{exact:true}).uncheck();
 await page.evaluate(()=>window.lose=true);
 await page.getByRole('button',{name:'Save schedule',exact:true}).click();
 await page.getByRole('button',{name:'Check schedule save',exact:true}).click();
 await page.getByText('Schedule saved.',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>saves[1].requestId),await page.evaluate(()=>saves[2].requestId));
 await page.getByRole('button',{name:'Edit schedule',exact:true}).click();
 await page.evaluate(()=>window.conflict=true);
 await page.getByRole('button',{name:'Save schedule',exact:true}).click();
 await page.getByText(/report version conflict/).waitFor();
 await page.getByRole('button',{name:'Cancel',exact:true}).click();
 await page.setViewportSize({width:390,height:844});
 await page.getByRole('button',{name:'Edit schedule',exact:true}).click();
 await page.screenshot({path:'/tmp/talia-web-schedule-mobile.png',fullPage:true});
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
 assert.deepEqual(errors,[]);
 console.log('Web schedules: cancel, daily/weekday save, pause, lost-response retry, version conflict, mobile layout passed.');
}finally{await browser.close();await new Promise(r=>server.close(r));}
