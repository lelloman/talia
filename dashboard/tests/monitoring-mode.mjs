// Real browser/fullscreen/DOM; dashboard selection is a fixture. access.mjs covers the live host.
import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {readFileSync,mkdirSync} from 'node:fs';
import http from 'node:http';import {once} from 'node:events';import path from 'node:path';import assert from 'node:assert/strict';
const root=process.cwd();let browser;
const server=http.createServer((req,res)=>{
 try{
  const url=new URL(req.url,'http://localhost');
  if(url.pathname==='/'){
   const html=readFileSync('dashboard/web/index.html','utf8').replace('<script type="module" src="/dashboard/web/auth.js"></script>',`<script type="module">
    import {mountChrome} from '/dashboard/web/dist/chrome.js';import {startMonitoringMode} from '/dashboard/web/monitoring-mode.js';
    await mountChrome({name:'Display test'},{development:true});
    window.dashboardReport={dirty:false};window.taliaDashboard={id:'first'};window.selections=[];
    window.talia={async selectDashboard(id){window.selections.push(id);await new Promise(r=>setTimeout(r,40));window.taliaDashboard={id};document.querySelector('#dashboard').textContent=id+' monitoring surface';window.dispatchEvent(new Event('talia-dashboard-loaded'));}};
    document.querySelector('#dashboard').textContent='first monitoring surface';
    window.display=startMonitoringMode({subject:'test-owner'});window.display.setCatalog([{id:'first'},{id:'second'}]);window.taliaShell.update({connected:true});
   </script>`);
   res.setHeader('Content-Type','text/html');res.end(html);return;
  }
  const file=path.resolve(root,'.'+url.pathname);if(!file.startsWith(root+'/'))throw Error('path');
  res.setHeader('Content-Type',file.endsWith('.js')?'text/javascript':file.endsWith('.css')?'text/css':file.endsWith('.svg')?'image/svg+xml':'application/octet-stream');res.end(readFileSync(file));
 }catch{res.statusCode=404;res.end();}
});
try{
 server.listen(0,'127.0.0.1');await once(server,'listening');const origin='http://127.0.0.1:'+server.address().port;
 browser=await chromium.launch({headless:true,channel:'chromium'});const context=await browser.newContext({viewport:{width:1280,height:800}}),p=await context.newPage();const errors=[];p.on('pageerror',e=>errors.push(e.message));
 await p.goto(origin);await p.waitForFunction(()=>!!window.display);
 await p.locator('.lv-sidebar a[href="#settings"]').click();
 await p.locator('#playlist-add-dashboard').selectOption('first');await p.locator('#playlist-add').click();
 await p.locator('#playlist-add-dashboard').selectOption('second');await p.locator('#playlist-add').click();
 await p.getByLabel('Seconds for first',{exact:true}).fill('5');await p.getByLabel('Seconds for first',{exact:true}).press('Tab');
 await p.getByLabel('Seconds for second',{exact:true}).fill('5');await p.getByLabel('Seconds for second',{exact:true}).press('Tab');
 await p.getByRole('button',{name:'Move up second',exact:true}).click();assert.match(await p.locator('#playlist-entries li').first().textContent(),/^second/);
 await p.getByRole('button',{name:'Move down second',exact:true}).click();await p.locator('#playlist-auto').check();
 await p.reload();await p.waitForFunction(()=>!!window.display);assert.equal(await p.locator('#playlist-auto').isChecked(),true);assert.equal(await p.locator('#playlist-entries li').count(),2);assert.equal(await p.evaluate(()=>!!document.fullscreenElement),false);
 await p.locator('.lv-sidebar a[href="#dashboard"]').click();await p.locator('#monitoring-enter').click();
 await p.waitForFunction(()=>document.fullscreenElement?.id==='monitoring-surface');
 const box=await p.locator('#monitoring-surface').boundingBox();assert.equal(box.width,1280);assert.equal(box.height,800);
 assert.equal(await p.locator('.workspace-bar').isVisible(),false);
 // Repeated unchanged catalog polls must not reset a long-running display's timer.
 await p.evaluate(()=>{window.catalogTimer=setInterval(()=>display.setCatalog([{id:'first'},{id:'second'}]),200);});
 await p.waitForFunction(()=>window.taliaDashboard.id==='second',{},{timeout:8000});
 await p.mouse.move(300,200);await p.locator('#monitoring-play').click();assert.equal(await p.locator('#monitoring-play').textContent(),'Play rotation');
 await p.locator('#monitoring-previous').click();await p.waitForFunction(()=>window.taliaDashboard.id==='first');
 // Dirty agent edits stop rotation and manual switching instead of being discarded.
 await p.evaluate(()=>window.dashboardReport.dirty=true);await p.locator('#monitoring-next').click();await p.getByText(/Rotation paused: temporary dashboard edits/).waitFor();assert.equal(await p.evaluate(()=>window.taliaDashboard.id),'first');
 await p.evaluate(()=>{window.dashboardReport.dirty=false;const d=document.querySelector('#diagnostic');d.hidden=false;d.textContent='Dashboard stopped — fixture failure';});
 await p.locator('#monitoring-play').click();await p.getByText('Rotation paused: the dashboard needs attention.',{exact:true}).waitFor();assert.equal(await p.locator('#diagnostic').isVisible(),true);
 await p.evaluate(()=>window.dispatchEvent(new CustomEvent('talia-connection',{detail:'disconnected'})));assert.equal(await p.locator('#monitoring-connection').textContent(),'disconnected');
 await p.evaluate(()=>{document.activeElement.blur();document.querySelector('#monitoring-surface').focus();});
 await p.waitForTimeout(3300);assert.equal(await p.locator('#monitoring-controls').evaluate(e=>getComputedStyle(e).opacity),'0');
 await p.mouse.move(301,201);await p.waitForFunction(()=>getComputedStyle(document.querySelector('#monitoring-controls')).opacity==='1');
 mkdirSync('.local/monitoring-mode',{recursive:true});await p.screenshot({path:'.local/monitoring-mode/fullscreen.png'});
 await p.keyboard.press('Escape');await p.waitForFunction(()=>!document.querySelector('#monitoring-surface').classList.contains('monitoring-active'));assert.equal(await p.locator('.workspace-bar').isVisible(),true);
 // Denied/unsupported native fullscreen still gives a usable, escapable monitoring surface.
 await p.evaluate(()=>{document.querySelector('#monitoring-surface').requestFullscreen=async()=>{throw Error('denied');};});
 await p.locator('#monitoring-enter').click();await p.getByText('Browser fullscreen is unavailable. Monitoring mode is active in this window.',{exact:true}).waitFor();
 assert.equal(await p.locator('.lv-sidebar').evaluate(e=>e.inert),true);await p.locator('#monitoring-exit').click();assert.equal(await p.locator('.lv-sidebar').evaluate(e=>e.inert),false);
 // Revoked playlist entries are retained as unavailable settings, never selected.
 await p.evaluate(()=>{clearInterval(window.catalogTimer);window.display.setCatalog([{id:'first'}]);});
 await p.locator('.lv-sidebar a[href="#settings"]').click();await p.getByText('second (unavailable — skipped)',{exact:true}).waitFor();
 await p.setViewportSize({width:390,height:844});assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
 await p.evaluate(()=>{location.hash='#dashboard';});await p.locator('#monitoring-enter').click();
 const controlsBox=await p.locator('#monitoring-controls').boundingBox();assert.ok(controlsBox.x>=0&&controlsBox.x+controlsBox.width<=390);
 await p.keyboard.press('Escape');
 assert.deepEqual(errors,[]);console.log('Monitoring mode browser checks passed: fullscreen, fallback, saved ordered playlist, timed/manual navigation, dirty/error guards, revocation and responsive controls');
}finally{await browser?.close();server.close();}
