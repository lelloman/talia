import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {spawn} from 'node:child_process';import {once} from 'node:events';import {mkdtemp,rm} from 'node:fs/promises';import os from 'node:os';import assert from 'node:assert/strict';
const temp=await mkdtemp(os.tmpdir()+'/talia-p2-web-'),bin=process.argv[2]||'engine/target/debug/talia-engine';let engine,server,browser,enginePort=0,info;
async function start(){engine=spawn(bin,[temp+'/engine.db',String(enginePort),'--seed'],{stdio:['ignore','pipe','inherit']});info=JSON.parse(String((await once(engine.stdout,'data'))[0]));enginePort=info.port;}
async function stop(){if(engine?.exitCode===null){engine.kill('SIGKILL');await once(engine,'exit');}}
try{
 await start();server=spawn('python3',['dashboard/serve.py','0'],{env:{...process.env,TALIA_ENGINE_DB:temp+'/engine.db',TALIA_ENGINE_PORT:String(enginePort)},stdio:['ignore','pipe','inherit']});const {port}=JSON.parse(String((await once(server.stdout,'data'))[0]));
 browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:900,height:900}});page.setDefaultTimeout(20000);const errors=[];page.on('pageerror',e=>{errors.push(String(e));process.stderr.write(String(e)+'\n')});
 await page.goto(`http://127.0.0.1:${port}`);await page.waitForFunction(()=>window.dashboardReport?.state.history.length>0).catch(async e=>{console.error(await page.locator('body').innerText(),await page.evaluate(()=>window.dashboardReport));throw e;});
 await page.getByRole('button',{name:'Controls',exact:true}).click();
 await page.evaluate(()=>talia.live("TaliaVM.replaceAction('special',(c)=>{const s=c.state();c.commit(s,{...s.value,value:Infinity,title:{text:undefined}})});TaliaVM.dispatch('special',{target:'test',value:null})"));
 await page.waitForFunction(()=>dashboardReport.dirty&&dashboardReport.state.value===Infinity);
 await page.getByText('Server value: ∞ (unavailable)',{exact:true}).waitFor();assert.equal(await page.getByRole('slider').isDisabled(),true);
 await page.getByRole('button',{name:'Apply value',exact:true}).click();await page.waitForFunction(()=>dashboardReport.actions.some(a=>a.status==='complete'));
 await stop();await page.getByText('disconnected',{exact:true}).waitFor({timeout:12000});await start();await page.getByText('back online',{exact:true}).waitFor({timeout:12000});
 assert.equal(await page.evaluate(()=>dashboardReport.dirty),true);assert.equal(await page.evaluate(()=>dashboardReport.state.screen),'controlsScreen');
 await page.getByRole('button',{name:'Overview',exact:true}).click();await page.getByRole('img',{name:/Observed server values.*∞/}).waitFor();
 await page.getByText('undefined',{exact:true}).waitFor();
 await page.getByRole('button',{name:'Reload dashboard',exact:true}).click();await page.waitForFunction(()=>dashboardReport?.dirty===false&&dashboardReport.state.history.some(v=>v===Infinity));
 await page.evaluate(()=>talia.live("throw Error('qualification failure')"));await page.getByRole('button',{name:'Restart dashboard',exact:true}).waitFor();
 await stop();await page.getByText('disconnected',{exact:true}).waitFor({timeout:12000});await start();await page.getByText('back online',{exact:true}).waitFor({timeout:12000});
 await page.getByRole('button',{name:'Restart dashboard',exact:true}).click();await page.waitForFunction(()=>!dashboardReport.failure&&dashboardReport.state.history.some(v=>v===Infinity));
 assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,checks:['live durable server','exceptional values through guest and SQLite','unavailable slider','chart exceptional annotation','visible undefined','server restart retains dirty VM and screen','connection states','saved baseline reload','connection survives dashboard failure'],state:await page.evaluate(()=>dashboardReport)}));
}finally{await browser?.close();if(server?.exitCode===null){server.kill('SIGTERM');await once(server,'exit');}await stop();await rm(temp,{recursive:true,force:true});}
