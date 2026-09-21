import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import assert from 'node:assert/strict';
const browser=await chromium.launch({headless:true});
try {
 const page=await browser.newPage();
 await page.goto(`http://127.0.0.1:${process.argv[2]}`);
 await page.waitForFunction(()=>window.dashboardReport?.registration?.connected);
 await page.evaluate(()=>talia.selectDashboard('authored'));
 await page.waitForFunction(()=>dashboardReport.client.dashboardId==='authored'&&dashboardReport.state.n===0);
 await page.getByRole('button',{name:'Increment',exact:true}).click();
 await page.waitForFunction(()=>dashboardReport.state.n===2);
 assert.equal(await page.evaluate(()=>dashboardReport.failure),null);
 assert.equal(await page.evaluate(()=>dashboardReport.dirty),false);
 assert.match(await page.evaluate(()=>dashboardReport.revision),/^catalog-/);
 console.log(JSON.stringify({passed:true,checks:['MCP-authored saved package on web','shared UI button invokes shared VM/function','clean runtime after authored interaction']}));
} finally {await browser.close();}
