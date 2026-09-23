import {readFileSync,mkdirSync} from 'node:fs';
import vm from 'node:vm';import assert from 'node:assert/strict';
import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {spawn} from 'node:child_process';import {once} from 'node:events';
import '../shared/ui.js';
const dir='dashboard/examples/homelab/';let model;
vm.runInNewContext(readFileSync(dir+'monitor.vm.js','utf8'),{defineVM:x=>model=x});
let state=model.initial();const ctx={state:()=>({revision:1,value:state}),commit:(_,next)=>state=next};
const data={updated:Date.now(),cpu:15.5,memory:20.0,cpuHistory:[14,12,18,13,15,14,13,17,16,14,15,16,15.5],memoryHistory:[19,19.4,19.7,20,20.1,19.8,19.9,20.2,20.1,19.9,20,19.8,20],disks:[{id:'root',mount:'/',device:'/dev/nvme0n1p2',free:68.7},{id:'boot',mount:'/boot/efi',device:'/dev/nvme0n1p1',free:99.4},{id:'external',mount:'/mnt/external',device:'/dev/sdb1',free:87.3}],targets:['Access Gateway','DNS Collector','Idle Manager','Knot Observer','Knot Resolver','LelloAuth','Prometheus','Host exporter','Observo','Pezzottflix','Pezzottify','Simple Agents','SimpleAI'].map((name,i)=>({id:String(i),name,instance:name.toLowerCase().replaceAll(' ','-')+':9090',status:'up'}))};
const snapshot=(quality='good')=>model.actions.snapshot(ctx,{value:{quality,value:data}});snapshot();
const ui=TaliaUI.compile(readFileSync(dir+'monitor.ui','utf8')),definitions={'homelab-metric-card':TaliaUI.compileDefinition(readFileSync(dir+'metric-card.ui','utf8'))};
for(const width of [340,600,900,1600])TaliaUI.resolve(ui,state,{width,definitions});
const server=spawn('python3',['-u','-m','http.server','0','--bind','127.0.0.1'],{stdio:['ignore','pipe','ignore']});let browser;
try{
 const port=Number(String((await once(server.stdout,'data'))[0]).match(/port (\d+)/)[1]);const origin=`http://127.0.0.1:${port}`;
 browser=await chromium.launch({headless:true});const page=await browser.newPage();
 await page.route('**/preview',r=>r.fulfill({contentType:'text/html',body:'<html><head><link rel="stylesheet" href="/dashboard/web/dist/chrome.css"><link rel="stylesheet" href="/dashboard/web/style.css"></head><body><div id="app"></div><template id="host-content"><section data-shell-page="dashboard"><div class="workspace-bar"><h1>Homelab overview</h1></div><div id="canvas" class="dashboard-canvas"></div></section><div id="agent-access" hidden></div></template><script src="/dashboard/shared/ui.js"></script></body></html>'}));
 await page.goto(origin+'/preview');await page.evaluate(async()=>{const {mountChrome}=await import('/dashboard/web/dist/chrome.js');await mountChrome({name:'Operator'},{development:true});window.renderer=new (await import('/dashboard/web/renderer.js')).Renderer(document.querySelector('#canvas'),()=>{});});
 async function render(){await page.evaluate(({ui,definitions,state})=>renderer.render(TaliaUI.resolve(ui,state,{definitions,width:document.querySelector('#canvas').clientWidth})),{ui,definitions,state:JSON.parse(JSON.stringify(state))});}
 mkdirSync('.local/monitor-redesign',{recursive:true});
 for(const width of [1920,1280,390]){
  await page.setViewportSize({width,height:width===390?844:1080});await render();
  assert.equal(await page.evaluate(()=>document.querySelector('.lv-main').scrollWidth<=document.querySelector('.lv-main').clientWidth),true);
  assert.equal(await page.locator('[data-node-id*="/name"]').filter({visible:true}).count(),13);
  assert.equal(await page.locator('figcaption').filter({visible:true}).first().textContent(),'Past hour · 5-minute samples');
  await page.screenshot({path:'.local/monitor-redesign/'+width+'-light.png',fullPage:true,animations:'disabled'});
 }
 await page.setViewportSize({width:1280,height:1080});await render();
 await page.getByRole('button',{name:/Change theme/}).click();await page.getByRole('button',{name:'Dark',exact:true}).click();
 await page.screenshot({path:'.local/monitor-redesign/1280-dark.png',fullPage:true,animations:'disabled'});
 data.targets[10].status='down';data.disks[0].free=6;data.cpu=null;snapshot();assert.equal(state.targets[0].name,'Pezzottify');await render();
 assert.equal(await page.getByText('● Unreachable',{exact:true}).count(),1);assert.equal(await page.getByText('Unavailable',{exact:true}).count(),1);
 await page.screenshot({path:'.local/monitor-redesign/degraded.png',fullPage:true,animations:'disabled'});
 snapshot('stale');assert.equal(state.statusTone,'warning');assert.ok(state.targets.every(t=>t.tone==='muted'));await render();
 assert.equal(await page.getByText('Last known: down',{exact:true}).count(),1);
 console.log(JSON.stringify({passed:true,checks:['1920/1280/390px layouts','light/dark','service grid','compact accessible trends','problem-first ordering','low disk/missing CPU','stale quality']}));
}finally{await browser?.close();server.kill('SIGTERM');}
