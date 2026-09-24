import {readFileSync} from 'node:fs';import vm from 'node:vm';import assert from 'node:assert/strict';
import '../shared/ui.js';import {changes,hosts} from '../../deploy/host-dashboards.mjs';
const source=id=>changes.find(c=>c.key.id===id).document.source;
const present=vm.runInNewContext('('+source('host-present')+')');
const collect=vm.runInNewContext('('+source('host-collect')+')');
assert.equal(changes.find(c=>c.key.kind==='monitor_definition'&&c.key.id==='host-collect').document.version,3);
const disk={metric:{device:'/dev/sda1',mountpoint:'/'},samples:[[1,68.7]]};
for(const p of hosts){
 const queries=[];let published;
 const total=16*1073741824,available=12*1073741824;
 const ctx={params:p,now:()=>1790183815800,source:async(_,q)=>{queries.push(q);return {result:q.kind==='range'?[{samples:[[q.start+300,20],[q.end,22]]}]:q.query.startsWith('up')?[{samples:[[1,1]]}]:q.query.includes('filesystem')?[disk]:q.query.startsWith('node_memory_MemTotal_bytes')?[{samples:[[1,total]]}]:q.query.startsWith('node_memory_MemAvailable_bytes')?[{samples:[[1,available]]}]:[{samples:[[1,20],[2,22]]}]};},publish:async(key,v)=>{assert.equal(key,'summary');published=v;}};
 await collect.run(ctx);
 assert.equal(queries.length,7);for(const q of queries){assert.ok(q.query.includes('job='+JSON.stringify(p.job)));assert.ok(q.query.includes('instance='+JSON.stringify(p.instance)));}
 for(const q of queries.filter(q=>q.kind==='range')){assert.equal(q.end-q.start,86400);assert.equal(q.step,300);}
 assert.equal(published.cpuHistory.length,289);assert.equal(published.cpuHistory[0],null);assert.equal(published.cpuHistory[1],20);assert.equal(published.cpuHistory[2],null);assert.equal(published.cpuHistory[288],22);
 assert.equal(published.memoryHistory.length,289);
 assert.equal(published.host,p.host);assert.equal(published.cpu,20);assert.equal(published.memory,25);assert.equal(published.memoryUsedBytes,4*1073741824);assert.equal(published.memoryTotalBytes,total);assert.equal(published.memoryAvailableBytes,available);assert.equal(published.disks[0].free,68.7);
 const state={ready:true,presentation:present({quality:'good',value:published},p.host)};
 assert.equal(state.presentation.memory.detail,'Used 4.0 of 16.0 GiB · 12.0 GiB available');
 const doc=changes.find(c=>c.key.kind==='dashboard'&&c.key.id===p.host).document;
 assert.deepEqual(doc.grants.reads,['host-'+p.host]);
 const ui=TaliaUI.compile(doc.ui),definitions={'host-layout':TaliaUI.compileDefinition(source('host-layout')),'host-metric-card':TaliaUI.compileDefinition(readFileSync('dashboard/examples/host/metric-card.ui','utf8'))};
 for(const width of [360,800,1600])TaliaUI.resolve(ui,JSON.parse(JSON.stringify(state)),{width,definitions});
 const stale=present({quality:'stale',value:published},p.host);assert.equal(stale.cpu.tone,'muted');assert.match(stale.status,/last known/);
 const healthySource=ctx.source;ctx.source=async(name,q)=>q.query.startsWith('node_memory_MemAvailable_bytes')?{result:[]}:healthySource(name,q);await collect.run(ctx);
 assert.equal(published.memory,null);assert.equal(published.memoryUsedBytes,null);assert.equal(present({quality:'good',value:published},p.host).memory.detail,'Absolute usage unavailable');
 ctx.source=async(_,q)=>({result:q.query.startsWith('up')?[{samples:[[1,0]]}]:q.query.includes('filesystem')?[disk]:[{samples:[[1,99]]}]});await collect.run(ctx);
 assert.equal(published.cpu,null);assert.equal(published.memory,null);assert.equal(published.memoryUsedBytes,null);assert.equal(published.disks[0].free,null);
 assert.equal(present({quality:'good',value:published},p.host).statusTone,'error');
 const missing=present({quality:'unavailable',value:null},p.host);assert.equal(missing.cpu.value,'Unavailable');
}
// Actual shared VM dispatch: each instance subscribes only to its own summary.
for(const host of hosts){
 const messages=[];const context=vm.createContext({__send:r=>messages.push(JSON.parse(r))});
 vm.runInContext(readFileSync('dashboard/shared/vm.js','utf8'),context);
 vm.runInContext('defineFunction("host-present",('+source('host-present')+'));'+readFileSync('dashboard/examples/host/dashboard.vm.js','utf8'),context);
 vm.runInContext('TaliaVM.start('+JSON.stringify({host:host.host,summary:'host-'+host.host})+')',context);
 await new Promise(r=>setImmediate(r));assert.equal(messages[0].op,'subscribe');assert.equal(messages[0].value,'host-'+host.host);
}
console.log('PASS: three host instances, isolated selectors/grants/subscriptions, shared responsive UI, stale/missing/down states');
