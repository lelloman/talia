import {readFileSync,mkdirSync} from 'node:fs';import vm from 'node:vm';import assert from 'node:assert/strict';
import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';import {spawn} from 'node:child_process';import {once} from 'node:events';
import '../shared/ui.js';import {changes,hosts} from '../../deploy/host-dashboards.mjs';
const source=id=>changes.find(c=>c.key.id===id).document.source,present=vm.runInNewContext('('+source('host-present')+')');
const definitions={'host-layout':TaliaUI.compileDefinition(source('host-layout')),'host-metric-card':TaliaUI.compileDefinition(readFileSync('dashboard/examples/host/metric-card.ui','utf8'))};
const server=spawn('python3',['-u','-m','http.server','0','--bind','127.0.0.1'],{stdio:['ignore','pipe','ignore']});let browser;
try{
 const port=Number(String((await once(server.stdout,'data'))[0]).match(/port (\d+)/)[1]);const origin=`http://127.0.0.1:${port}`;
 browser=await chromium.launch({headless:true});const page=await browser.newPage();
 await page.route('**/preview',r=>r.fulfill({contentType:'text/html',body:'<html><head><link rel="stylesheet" href="/dashboard/web/dist/chrome.css"><link rel="stylesheet" href="/dashboard/web/style.css"></head><body><div id="app"></div><template id="host-content"><section data-shell-page="dashboard"><div id="canvas" class="dashboard-canvas"></div></section><div id="agent-access" hidden></div></template><script src="/dashboard/shared/ui.js"></script></body></html>'}));
 await page.goto(origin+'/preview');await page.evaluate(async()=>{await (await import('/dashboard/web/dist/chrome.js')).mountChrome({name:'Operator'},{development:true});window.renderer=new (await import('/dashboard/web/renderer.js')).Renderer(document.querySelector('#canvas'),()=>{});});
 mkdirSync('.local/host-template',{recursive:true});
 for(const [i,p] of hosts.entries()){
  const sample={quality:'good',value:{updated:Date.now(),reachable:true,cpu:12+i*10,memory:100*(4+i)/16,memoryUsedBytes:(4+i)*1073741824,memoryTotalBytes:16*1073741824,memoryAvailableBytes:(12-i)*1073741824,cpuHistory:[10,14,12],cpuMinuteHistory:[10,90,12],memoryHistory:[18,19,25+i*6.25],disks:[{id:'root',mount:'/',device:'/dev/sda1',free:70-i*10}]}};
  const state=JSON.parse(JSON.stringify({ready:true,presentation:present(sample,p.host)}));const ui=TaliaUI.compile(changes.find(c=>c.key.kind==='dashboard'&&c.key.id===p.host).document.ui);
  for(const width of [390,1280]){
   await page.setViewportSize({width,height:900});await page.evaluate(({ui,definitions,state})=>renderer.render(TaliaUI.resolve(ui,state,{definitions,width:document.querySelector('#canvas').clientWidth})),{ui,definitions,state});
   await page.getByRole('heading',{name:p.host,exact:true}).waitFor();assert.equal(await page.evaluate(()=>document.querySelector('.lv-main').scrollWidth<=document.querySelector('.lv-main').clientWidth),true);
   assert.equal(await page.locator('figcaption').filter({visible:true}).count(),2);assert.equal(await page.locator('figcaption').filter({visible:true}).first().textContent(),'Past 24 hours · 5-minute CPU averages');
   assert.equal(await page.locator('figure canvas').filter({visible:true}).count(),2);
   assert.equal(await page.getByRole('button',{name:'Show past 24 hours of CPU',pressed:true}).count(),1);
   await page.waitForFunction(()=>[...document.querySelectorAll('figure canvas')].filter(c=>c.getBoundingClientRect().width>0).every(c=>{const r=c.getBoundingClientRect(),d=devicePixelRatio;return Math.abs(c.width-r.width*d)<=1&&Math.abs(c.height-r.height*d)<=1;}));
   assert.equal(await page.evaluate(()=>[...document.querySelectorAll('figure')].filter(e=>e.getBoundingClientRect().width>0).every(e=>{const canvas=e.querySelector('canvas').getBoundingClientRect(),caption=e.querySelector('figcaption').getBoundingClientRect(),next=e.nextElementSibling?.getBoundingClientRect();return canvas.bottom<=caption.top&&(!next||caption.bottom<=next.top)})),true);
   await page.getByText(`Used ${(4+i).toFixed(1)} of 16.0 GiB · ${(12-i).toFixed(1)} GiB available`,{exact:true}).waitFor();
   await page.screenshot({path:'.local/host-template/'+p.host+'-'+width+'.png',fullPage:true});
   if(i===0&&width===390){const hourState=JSON.parse(JSON.stringify({ready:true,presentation:present(sample,p.host,'hour')}));await page.evaluate(({ui,definitions,state})=>renderer.render(TaliaUI.resolve(ui,state,{definitions,width:document.querySelector('#canvas').clientWidth})),{ui,definitions,state:hourState});await page.getByText('Past hour · 1-minute CPU averages',{exact:true}).waitFor();assert.equal(await page.getByRole('button',{name:'Show past hour of CPU',pressed:true}).count(),1);await page.screenshot({path:'.local/host-template/'+p.host+'-hour-390.png',fullPage:true});}
  }
 }
 await page.getByRole('button',{name:/Change theme/}).click();await page.getByRole('button',{name:'Dark',exact:true}).click();
 await page.screenshot({path:'.local/host-template/vps-us-dark-1280.png',fullPage:true});
 console.log('PASS: all three host dashboards at 390px and 1280px');
}finally{await browser?.close();server.kill();}
