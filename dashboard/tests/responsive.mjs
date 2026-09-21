import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';import {spawn} from 'node:child_process';import {once} from 'node:events';import assert from 'node:assert/strict';
const server=spawn('python3',['dashboard/serve.py','0'],{stdio:['ignore','pipe','inherit']});let browser,page;
try{
 const {port}=JSON.parse(String((await once(server.stdout,'data'))[0]));browser=await chromium.launch({headless:true});page=await browser.newPage({viewport:{width:1300,height:900}});await page.goto(`http://127.0.0.1:${port}`);await page.waitForFunction(()=>window.dashboardReport?.state.history.length>0);
 await page.getByText('Expanded monitoring surface',{exact:true}).waitFor();await page.getByRole('button',{name:'Controls',exact:true}).click();await page.getByRole('switch',{name:'Show details'}).click();
 await page.locator('.lv-sidebar a[href="#settings"]').click();await page.locator('#dp-scale').fill('2');await page.locator('#dp-scale').dispatchEvent('change');await page.locator('#sidebar').check();await page.waitForFunction(()=>dashboardReport.client.params.sidebar===true);
 assert.equal(await page.evaluate(()=>dashboardReport.state.details),false);assert.equal(await page.evaluate(()=>dashboardReport.state.screen),'controlsScreen');
 await page.locator('.lv-sidebar a[href="#dashboard"]').click();await page.getByRole('button',{name:'Overview',exact:true}).click();await page.getByText('Compact monitoring surface',{exact:true}).waitFor();
 await page.reload();await page.waitForFunction(()=>window.dashboardReport?.client.scale===2);assert.equal(await page.locator('#sidebar').isChecked(),true);
 const result=await page.evaluate(async()=>{
  const {Renderer}=await import('/dashboard/web/renderer.js');const target=document.createElement('div');document.body.append(target);const r=new Renderer(target,()=>{});
  const source='<Dashboard id="d"><Surface id="s"><Column id="root"><For id="list" items={state.items} key={item.id}><Switch id="switch" label={item.label} value={item.value} onChange={actions.change}/></For><Text id="hidden" text="Hidden" visibility="hidden"/><Text id="collapsed" text="Collapsed" visibility="collapsed"/></Column></Surface></Dashboard>';
  const tree=TaliaUI.compile(source),a={id:'a',label:'A',value:true},b={id:'b',label:'B',value:false};r.render(TaliaUI.resolve(tree,{items:[a,b]}));const input=target.querySelector('input');input.focus();r.render(TaliaUI.resolve(tree,{items:[b,a]}));
  return {same:target.querySelectorAll('input')[1]===input,focused:document.activeElement===input,hidden:target.querySelector('[data-node-id$="/hidden"]').getBoundingClientRect().height,collapsed:target.querySelector('[data-node-id$="/collapsed"]').getBoundingClientRect().height};
 });assert.equal(result.same,true);assert.equal(result.focused,true);assert.ok(result.hidden>0);assert.equal(result.collapsed,0);
 console.log(JSON.stringify({passed:true,checks:['width breakpoints','manual dp scale persisted per client','composition preserves state/navigation','keyed DOM identity and focus','hidden versus collapsed'],result}));
}catch(e){console.error(await page?.evaluate(()=>({diagnostic:document.querySelector('#diagnostic')?.textContent,report:window.dashboardReport,storage:localStorage.getItem('talia.client')})));throw e;}finally{await browser?.close();server.kill('SIGTERM');}
