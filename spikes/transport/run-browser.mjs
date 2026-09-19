import {chromium} from '../runtime/node_modules/playwright/index.mjs';
const port=Number(process.argv[2]);if(!port)throw Error('Pass loopback server port');
const browser=await chromium.launch({headless:true});
try {
 const page=await browser.newPage();await page.goto(`http://127.0.0.1:${port}/`);
 await page.waitForFunction(()=>window.transportResult,null,{timeout:65000});
 const report=await page.evaluate(()=>window.transportResult);report.browser=browser.version();
 console.log(JSON.stringify(report));if(report.error || !report.done || report.renderer_ticks<1)process.exitCode=1;
}finally{await browser.close();}
