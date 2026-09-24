import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import assert from 'node:assert/strict';

const server=spawn('python3',['dashboard/serve.py','0'],{stdio:['ignore','pipe','inherit']});
let browser;
try{
 const port=JSON.parse(String((await once(server.stdout,'data'))[0])).port;
 browser=await chromium.launch({headless:true});
 const page=await browser.newPage();
 await page.goto(`http://127.0.0.1:${port}/`);
 await page.waitForFunction(()=>window.dashboardReport?.revision&&window.talia?.state);
 const first=await page.evaluate(()=>talia.registration().liveInstanceId);
 await page.evaluate(()=>talia.live('void 0'));
 await page.waitForFunction(()=>dashboardReport.dirty===true);
 await page.evaluate(()=>talia.selectDashboard('monitoring'));
 await page.waitForFunction(()=>dashboardReport.client.dashboardId==='monitoring');
 await page.evaluate(()=>talia.selectDashboard('monitor'));
 await page.waitForFunction(()=>dashboardReport.client.dashboardId==='monitor'&&dashboardReport.dirty===true);
 assert.notEqual(await page.evaluate(()=>talia.registration().liveInstanceId),first);
 await page.evaluate(()=>talia.reload());
 await page.waitForFunction(()=>dashboardReport.client.dashboardId==='monitor'&&dashboardReport.dirty===false);
 console.log('Paused dashboard keeps ViewModel state across switches; Reload resets it.');
}finally{
 await browser?.close();server.kill('SIGTERM');await once(server,'exit');
}
