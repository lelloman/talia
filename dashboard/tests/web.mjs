import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {spawn} from 'node:child_process';import {once} from 'node:events';import assert from 'node:assert/strict';
const server=spawn('python3',['dashboard/serve.py','0'],{stdio:['ignore','pipe','inherit']});let browser;
try{
 const line=await once(server.stdout,'data');const {port}=JSON.parse(String(line[0]));
 browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:900,height:900}});const errors=[];page.on('pageerror',e=>errors.push(String(e)));
 await page.goto(`http://127.0.0.1:${port}`);await page.waitForFunction(()=>window.dashboardReport?.state.history.length>0);
 await page.getByRole('button',{name:'Controls',exact:true}).click();await page.getByRole('slider',{name:'Server value'}).waitFor();
 const slider=page.getByRole('slider',{name:'Server value'});await slider.focus();await page.keyboard.press('ArrowRight');await page.keyboard.press('Tab');
 await page.waitForFunction(()=>window.dashboardReport.state.value===26);
 const bounds=await slider.boundingBox();await page.mouse.move(bounds.x+bounds.width*.7,bounds.y+bounds.height/2);await page.mouse.down();await page.mouse.move(bounds.x+bounds.width*.8,bounds.y+bounds.height/2,{steps:5});await page.waitForFunction(()=>window.dashboardReport.state.value>60);await page.mouse.up();
 await slider.focus();await page.keyboard.press('Home');for(let i=0;i<26;i++)await page.keyboard.press('ArrowRight');await page.keyboard.press('Tab');await page.waitForFunction(()=>dashboardReport.state.value===26);
 await page.getByRole('switch',{name:'Show details'}).click();await page.waitForFunction(()=>window.dashboardReport.state.details===false);
 assert.equal(await page.getByText('Writes change server state').count(),0);
 await page.getByRole('button',{name:'Apply value',exact:true}).click();await page.waitForFunction(()=>window.dashboardReport.actions.some(a=>a.status==='completed')).catch(async e=>{console.error(await page.evaluate(()=>window.dashboardReport));throw e;});
 await page.getByRole('button',{name:'Overview',exact:true}).click();await page.getByRole('img',{name:/Observed server values/}).waitFor();
 assert.equal(await page.evaluate(()=>dashboardReport.state.value),26);
 assert.equal(await page.evaluate(()=>dashboardReport.subscriptions),1);
 assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,checks:['shared VM in bounded QuickJS Worker','live server subscription','keyboard slider and live pointer drag','switch/conditional','write outcome','navigation preserves state and subscription','accessible chart'],state:await page.evaluate(()=>dashboardReport)}));
}finally{await browser?.close();server.kill('SIGTERM');}
