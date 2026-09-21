import {chromium} from '../../spikes/runtime/node_modules/playwright/index.mjs';
import {spawn} from 'node:child_process';import {createInterface} from 'node:readline';import assert from 'node:assert/strict';
const [engine,port,token,denied]=process.argv.slice(2);
class MCP {
 constructor(file){this.seq=0;this.pending=new Map();this.p=spawn('/tmp/talia-p3-target/debug/talia-mcp',['http://127.0.0.1:'+engine,file]);this.p.stderr.pipe(process.stderr);createInterface({input:this.p.stdout}).on('line',s=>{const r=JSON.parse(s);this.pending.get(r.id)?.(r);this.pending.delete(r.id);});}
 rpc(method,params){const id=++this.seq;return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{this.pending.delete(id);reject(Error('MCP response timeout'));},15000);timer.unref();this.pending.set(id,r=>{clearTimeout(timer);resolve(r);});this.p.stdin.write(JSON.stringify({jsonrpc:'2.0',id,method,params})+'\n');});}
 async init(){await this.rpc('initialize',{protocolVersion:'2025-11-25',capabilities:{},clientInfo:{name:'live-test',version:'1'}});this.p.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/initialized'})+'\n');}
 async tool(name,args={},ok=true){const r=await this.rpc('tools/call',{name,arguments:args});assert.equal(!!r.result?.isError,!ok,JSON.stringify(r));return r.result.structuredContent;}
 close(){this.p.stdin.end();}
}
const m=new MCP(token),limited=new MCP(denied),browser=await chromium.launch({headless:true});
const eventually=async fn=>{const end=Date.now()+10000;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,100));}throw Error('condition timeout');};
try{
 await m.init();await limited.init();const context=await browser.newContext(),page=await context.newPage();await page.goto('http://127.0.0.1:'+port);await page.waitForFunction(()=>window.dashboardReport?.registration?.connected);
 const address=async()=>page.evaluate(()=>{const r=talia.registration();return {clientId:r.clientId,slotId:r.slotId,liveInstanceId:r.liveInstanceId};});let target=await address();
 const catalog=await m.tool('definitions_list');const before=await m.tool('live_inspect',{target});assert.equal(before.snapshot.dirty,false);
 const other=await context.newPage();await other.goto('http://127.0.0.1:'+port);await other.waitForFunction(()=>window.dashboardReport?.registration?.connected);
 assert((await m.tool('clients_list')).clients[0].slots.length===2);
 const source='if(typeof TaliaVM!=="undefined"||typeof document!=="undefined")throw Error("isolation");const s=ctx.state();await ctx.commit(s,{...s.value,title:{text:"Edited live"},value:Infinity});await ctx.write("value",77);';
 const args={target,expectedEditRevision:0,source,requestId:'edit-web'};const edit=await m.tool('live_execute',args);assert.equal(edit.audit.status,'complete',JSON.stringify(edit));assert.equal(edit.outcome.editRevision,1);
 assert.deepEqual(await m.tool('live_execute',args),edit);assert.equal((await m.tool('live_execute',{...args,source:'await ctx.write("value",555)'},false)).error,'conflict');await page.waitForFunction(()=>window.dashboardReport?.state?.value===Infinity&&dashboardReport.dirty);
 assert.equal(await other.evaluate(()=>dashboardReport.dirty),false);assert.equal((await m.tool('engine_read',{id:'value'})).sample.value.value[1],77);
 assert.equal((await m.tool('definitions_list')).catalogRevision,catalog.catalogRevision);
 assert.equal((await m.tool('live_execute',{...args,requestId:'stale-edit'},false)).error,'conflict');
 const status=await m.tool('clients_list');const slot=status.clients[0].slots.find(s=>s.liveInstanceId===target.liveInstanceId);
 const reload={target,expectedEditRevision:1,expectedAssignmentRevision:slot.desiredAssignment.revision,requestId:'reload-web'};
 assert.equal((await m.tool('live_reload',reload,false)).error,'dirty_ack_required');
 const reloaded=await m.tool('live_reload',{...reload,requestId:'reload-ack',discardDirty:true});assert.equal(reloaded.audit.status,'complete',JSON.stringify(reloaded));
 const replacement=await address();assert.notEqual(replacement.liveInstanceId,target.liveInstanceId);assert.equal((await m.tool('live_inspect',{target},false)).error,'stale_instance');target=replacement;
 await page.waitForFunction(()=>!dashboardReport.dirty);assert.equal((await m.tool('engine_read',{id:'value'})).sample.value.value[1],77);
 assert.equal((await m.tool('operation_status',{requestId:'reload-ack'})).outcome.liveInstanceId,target.liveInstanceId);
 // A live-only principal cannot borrow the dashboard's engine grants.
 const forbidden=await limited.tool('live_execute',{target,expectedEditRevision:0,source:'await ctx.write("value",99);',requestId:'no-engine'},false);assert.equal(forbidden.audit.status,'failed');assert.equal((await m.tool('engine_read',{id:'value'})).sample.value.value[1],77);
 await page.waitForFunction(()=>!!dashboardReport.failure);assert.equal((await m.tool('live_inspect',{target})).snapshot.failure,'dashboard_failed');
 const restart=await m.tool('live_reload',{target,expectedEditRevision:1,expectedAssignmentRevision:slot.desiredAssignment.revision,discardDirty:true,requestId:'restart-failed'});assert.equal(restart.audit.status,'complete',JSON.stringify(restart));
 await page.waitForFunction(()=>dashboardReport.registration.connected&&!dashboardReport.failure);
 target=await address();
 const pending=m.tool('live_execute',{target,expectedEditRevision:0,source:'await ctx.write("value",88);await new Promise(()=>{});await ctx.write("value",999);',requestId:'cancel-live'},false);const pendingId=m.seq;
 await eventually(async()=> (await m.tool('engine_read',{id:'value'})).sample.value.value[1]===88);
 m.p.stdin.write(JSON.stringify({jsonrpc:'2.0',method:'notifications/cancelled',params:{requestId:pendingId}})+'\n');pending.catch(()=>{});
 await eventually(async()=> ['unknown','cancelled'].includes((await m.tool('operation_status',{requestId:'cancel-live'})).audit.status));
 assert.equal((await m.tool('engine_read',{id:'value'})).sample.value.value[1],88);
 await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));});
 await eventually(async()=>{const cs=await m.tool('clients_list');return cs.clients[0].slots.find(s=>s.liveInstanceId===target.liveInstanceId)?.report.lifecycle==='paused';});
 assert.equal((await m.tool('live_execute',{target,expectedEditRevision:0,source:'await ctx.write("value",88)',requestId:'paused'},false)).error,'target_unavailable');
 console.log(JSON.stringify({passed:true,checks:['scoped discovery and clean inspection','isolated live state and engine effects','exact retry without replay','revision guards and dirty acknowledgement','pinned reload replaces only one tab','engine permission intersection','failed guest inspection/restart','MCP cancellation preserves earlier effects','paused immediate rejection','saved definitions unchanged']}));
}finally{await browser.close();m.close();limited.close();}
