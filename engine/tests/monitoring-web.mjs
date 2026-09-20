import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {spawn} from 'node:child_process';import {createInterface} from 'node:readline';import {once} from 'node:events';import assert from 'node:assert/strict';
const fixture=spawn('python3',['engine/tests/monitoring_fixture.py',process.argv[2]||'/tmp/talia-p3-target/debug/talia-engine'],{stdio:['pipe','pipe','inherit']});
const lines=createInterface({input:fixture.stdout})[Symbol.asyncIterator]();const read=async()=>JSON.parse((await lines.next()).value);const command=async(op,value)=>{fixture.stdin.write(JSON.stringify({op,value})+'\n');return read();};let browser,page;
try{
 const {port}=await read();browser=await chromium.launch({headless:true});page=await browser.newPage({viewport:{width:950,height:1000}});page.setDefaultTimeout(20000);
 const errors=[];page.on('pageerror',e=>errors.push(String(e)));
 await page.addInitScript(()=>localStorage.setItem('talia.client',JSON.stringify({dashboardId:'monitoring'})));
 await page.goto(`http://127.0.0.1:${port}`);
 await page.waitForFunction(()=>window.dashboardReport?.state.samples.cpu?.value===42&&Object.keys(dashboardReport.state.samples).length===9&&dashboardReport.subscriptions===9).catch(async e=>{console.error(await page.locator('body').innerText(),await page.evaluate(()=>window.dashboardReport));throw e;});
 assert.equal(await page.evaluate(()=>dashboardReport.subscriptions),9);
 await page.getByText(/CPU: 42% · good/).waitFor();await page.getByText(/Disk X free: 12%/).waitFor();
 await command('disk',5);await page.getByText('Disk X: investigation triggered',{exact:true}).waitFor();await page.getByText('Disk Y: armed',{exact:true}).waitFor();
 await page.getByRole('button',{name:'Investigation',exact:true}).click();await page.getByText('db: 60%',{exact:true}).waitFor();
 await page.getByRole('button',{name:'Run investigation',exact:true}).click();await page.getByText('Investigation requested',{exact:true}).waitFor();
 for(let n=0;n<100;n++){if((await command('status')).probes>=2)break;await page.waitForTimeout(50);}assert.equal((await command('status')).probes,2);
 await page.evaluate(()=>talia.live("TaliaVM.replaceAction('mark',c=>{const s=c.state();c.commit(s,{...s.value,note:'temporary'})});TaliaVM.dispatch('mark',{target:'test',value:null})"));
 await command('stop');await page.getByText('disconnected',{exact:true}).waitFor();await command('restart');await page.getByText('back online',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>dashboardReport.state.note),'temporary');assert.equal(await page.evaluate(()=>dashboardReport.state.screen),'investigation');assert.equal((await command('status')).probes,2);
 await page.getByRole('button',{name:'Reload dashboard',exact:true}).click();await page.waitForFunction(()=>!dashboardReport.dirty&&dashboardReport.state.screen==='overview'&&dashboardReport.state.samples.cpu?.value===42&&Object.keys(dashboardReport.state.samples).length===9);
 await page.setViewportSize({width:390,height:900});await page.getByText('Live collection',{exact:true}).waitFor();assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,checks:['nine named subscriptions','metrics with age and quality','independent Watch flags','probe breakdown','manual run','restart preserves live state and no duplicate investigation','baseline reload','responsive layout'],state:await page.evaluate(()=>dashboardReport)}));
}catch(e){if(page)console.error(await page.locator('body').innerText(),JSON.stringify(await page.evaluate(()=>window.dashboardReport)));throw e;}finally{await browser?.close();fixture.stdin.end();if(fixture.exitCode===null)await once(fixture,'exit');}
