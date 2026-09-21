import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import assert from 'node:assert/strict';
const browser=await chromium.launch({headless:true});
try {
 const page=await browser.newPage();await page.goto(`http://127.0.0.1:${process.argv[2]}`);
 await page.waitForFunction(()=>window.dashboardReport?.state?.history?.some(v=>v===Infinity));
 await page.getByRole('img',{name:/Observed server values.*∞/}).waitFor();
 assert.equal(await page.evaluate(()=>dashboardReport.dirty),false);
 console.log(JSON.stringify({passed:true,checks:['MCP write observed by web subscription','exceptional value visible in chart','authored VM remains clean']}));
} finally {await browser.close();}
