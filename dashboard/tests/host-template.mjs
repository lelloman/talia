import {readFileSync} from 'node:fs';import vm from 'node:vm';import assert from 'node:assert/strict';
import '../shared/ui.js';import {changes,hosts} from '../../deploy/host-dashboards.mjs';
const source=id=>changes.find(c=>c.key.id===id).document.source;
const present=vm.runInNewContext('('+source('host-present')+')');
const collect=vm.runInNewContext('('+source('host-collect')+')');
assert.equal(changes.find(c=>c.key.kind==='monitor_definition'&&c.key.id==='host-collect').document.version,10);
const disk={metric:{device:'/dev/sda1',mountpoint:'/'},samples:[[1,68700000000]]};
for(const p of hosts){
 const queries=[];let published;
 const total=16*1073741824,available=12*1073741824;
 const ctx={params:p,now:()=>1790183815800,source:async(_,q)=>{queries.push(q);
 if(q.query.includes('network_')){
  if(q.query.includes('checked_timestamp'))return {result:[{samples:[[1,ctx.now()/1000]]}]};
  if(q.kind==='range')return {result:[{metric:{direction:'download'},samples:[[q.start+30,1200000],[q.end,2400000]]},{metric:{direction:'upload'},samples:[[q.start+30,300000],[q.end,600000]]}]};
  const days=q.query.includes('[30d]')?30:q.query.includes('[7d]')?7:1;
  return {result:[{metric:{direction:'download'},samples:[[1,123e9]]},{metric:{direction:'upload'},samples:[[1,45e9]]},{metric:{direction:'samples'},samples:[[1,(Math.min(days,3)*86400/30)+1]]}]};
 }
 return {result:q.kind==='range'?[{samples:[[q.start+q.step,q.query.startsWith('max_over_time')?100:20],[q.end,q.query.startsWith('max_over_time')?80:22]]}]:q.query.startsWith('up')?[{samples:[[1,1]]}]:q.query.startsWith('node_filesystem_size_bytes')?[{...disk,samples:[[1,100000000000]]}]:q.query.includes('filesystem')?[disk,{...disk,metric:{device:'/dev/sda2',mountpoint:'/boot'}},{...disk,metric:{device:'/dev/sda3',mountpoint:'/boot/efi'}},{...disk,metric:{device:'/dev/sda4',mountpoint:'/efi'}}]:q.query.startsWith('node_memory_MemTotal_bytes')?[{samples:[[1,total]]}]:q.query.startsWith('node_memory_MemAvailable_bytes')?[{samples:[[1,available]]}]:[{samples:[[1,20],[2,22]]}]};},publish:async(key,v)=>{assert.equal(key,'summary');published=v;}};
 await collect.run(ctx);
 assert.equal(queries.length,p.gitWorkspaces?17:15);for(const q of queries){assert.ok(q.query.includes('job='+JSON.stringify(p.job)));assert.ok(q.query.includes('instance='+JSON.stringify(p.instance)));}
 for(const q of queries.filter(q=>q.kind==='range'&&!q.query.includes('network_'))){assert.equal(q.end-q.start,(q.query.includes('[1m]')||q.query.includes('[1m:30s]'))?3600:86400);assert.equal(q.step,(q.query.includes('[1m]')||q.query.includes('[1m:30s]'))?60:300);}
 assert.equal(published.cpuHistory.length,289);assert.equal(published.cpuHistory[0],null);assert.equal(published.cpuHistory[1],20);assert.equal(published.cpuHistory[2],null);assert.equal(published.cpuHistory[288],22);
 assert.equal(published.network.device,p.networkDevice);assert.equal(published.network.downloadHistory.length,121);assert.equal(published.network.downloadHistory[0],null);assert.equal(published.network.download,2400000);assert.equal(published.network.upload,600000);assert.equal(published.network.totals[2].observedSeconds,3*86400);
 const networkState=present({quality:'good',value:published},p.host).network;assert.equal(networkState.download,'2.4 MB/s');assert.equal(networkState.unit,'MB/s');assert.equal(networkState.chartMax,5);assert.equal(networkState.totals[0].coverage,'Complete history');assert.match(networkState.totals[2].coverage,/Partial history/);
 for(const q of queries.filter(q=>q.query.includes('network_'))){assert.ok(q.query.includes('device='+JSON.stringify(p.networkDevice))||q.query.includes('checked_timestamp'));if(q.query.includes('increase(')){assert.ok(q.query.includes('sum(increase('));assert.ok(!q.query.includes('__name__=~'));assert.equal(q.query.split('count_over_time(').length-1,2);}}
 assert.equal(present({quality:'stale',value:published},p.host).network.download,'Unavailable');
 assert.equal(published.cpuPeakHistory.length,289);assert.equal(published.cpuPeakHistory[1],100);assert.equal(published.cpuPeakHistory[2],null);assert.equal(published.cpuMinutePeakHistory.length,61);assert.equal(published.cpuMinutePeakHistory[1],100);
 for(const q of queries.filter(q=>q.query.startsWith('max_over_time')))assert.ok(q.query.includes('avg(irate(')&&q.query.includes(':30s]'));
 assert.equal(published.memoryHistory.length,289);
 assert.equal(published.cpuMinuteHistory.length,61);assert.equal(published.cpuMinuteHistory[0],null);assert.equal(published.cpuMinuteHistory[1],20);assert.equal(published.cpuMinuteHistory[60],22);
 assert.equal(published.host,p.host);assert.equal(published.cpu,20);assert.equal(published.memory,25);assert.equal(published.memoryUsedBytes,4*1073741824);assert.equal(published.memoryTotalBytes,total);assert.equal(published.memoryAvailableBytes,available);assert.equal(published.disks[0].free,68.7);assert.equal(published.disks.length,1);assert.equal(published.disks[0].availableBytes,68700000000);assert.equal(published.disks[0].totalBytes,100000000000);
 const state={ready:true,presentation:present({quality:'good',value:published},p.host)};
 assert.equal(state.presentation.disks[0].detail,'68.7 GB available of 100.0 GB');
 assert.equal(state.presentation.memory.detail,'Used 4.0 of 16.0 GiB · 12.0 GiB available');
 const hour=present({quality:'good',value:published},p.host,'hour');assert.equal(hour.cpu.peakHistory[1],100);assert.equal(hour.cpu.history.length,61);assert.equal(hour.cpu.chartLabel,'Past hour · 1-minute CPU averages');assert.equal(hour.cpu.detail,'Current: 5-minute average across cores');assert.equal(hour.cpu.hourSelected,true);assert.equal(hour.cpu.daySelected,false);
 const doc=changes.find(c=>c.key.kind==='dashboard'&&c.key.id===p.host).document;
 assert.deepEqual(doc.grants.reads,['host-'+p.host,...(p.gitWorkspaces?['monitor.git-recheck-'+p.host]:[])]);
 const ui=TaliaUI.compile(doc.ui),definitions={'host-layout':TaliaUI.compileDefinition(source('host-layout')),'host-metric-card':TaliaUI.compileDefinition(readFileSync('dashboard/examples/host/metric-card.ui','utf8'))};
 for(const width of [360,800,1600])TaliaUI.resolve(ui,JSON.parse(JSON.stringify(state)),{width,definitions});
 const stale=present({quality:'stale',value:published},p.host);assert.equal(stale.cpu.tone,'muted');assert.match(stale.status,/last known/);
 const originalSource=ctx.source;
 ctx.source=async(name,q)=>q.query.startsWith('node_filesystem_size_bytes')?{result:[]}:originalSource(name,q);
 await collect.run(ctx);assert.equal(published.disks[0].free,null);assert.equal(published.disks[0].availableBytes,null);
 assert.equal(present({quality:'good',value:published},p.host).disks[0].detail,'Absolute capacity unavailable');
 ctx.source=async(name,q)=>{if(q.query.includes('network_'))throw Error('network unavailable');return originalSource(name,q);};
 await collect.run(ctx);assert.equal(published.cpu,20);assert.equal(published.memory,25);assert.equal(published.network.unavailable,true);
 ctx.source=async(name,q)=>q.query.includes('checked_timestamp')?{result:[{samples:[[1,ctx.now()/1000-120]]}]}:originalSource(name,q);
 await collect.run(ctx);if(p.networkHostProbe){assert.equal(published.network.download,null);assert.equal(published.network.fresh,false);}
 ctx.source=originalSource;
 const healthySource=ctx.source;ctx.source=async(name,q)=>q.query.startsWith('node_memory_MemAvailable_bytes')?{result:[]}:healthySource(name,q);await collect.run(ctx);
 assert.equal(published.memory,null);assert.equal(published.memoryUsedBytes,null);assert.equal(present({quality:'good',value:published},p.host).memory.detail,'Absolute usage unavailable');
 ctx.source=async(_,q)=>({result:q.query.startsWith('up')?[{samples:[[1,0]]}]:q.query.startsWith('node_filesystem_size_bytes')?[{...disk,samples:[[1,100000000000]]}]:q.query.includes('filesystem')?[disk,{...disk,metric:{device:'/dev/sda2',mountpoint:'/boot'}},{...disk,metric:{device:'/dev/sda3',mountpoint:'/boot/efi'}},{...disk,metric:{device:'/dev/sda4',mountpoint:'/efi'}}]:[{samples:[[1,99]]}]});await collect.run(ctx);
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
 context.TaliaVM.receive(JSON.stringify({id:messages[0].id,value:'subscription'}));await new Promise(r=>setImmediate(r));
 context.TaliaVM.receive(JSON.stringify({event:'subscription',value:{quality:'good',value:{reachable:true,cpu:20,cpuHistory:[10,20],cpuMinuteHistory:[10,30],memoryHistory:[]}}}));await new Promise(r=>setImmediate(r));assert.equal(context.TaliaVM.snapshot().state.ready,true,JSON.stringify(context.TaliaVM.snapshot()));
 vm.runInContext("TaliaVM.dispatch('cpuHour',{})",context);await new Promise(r=>setImmediate(r));assert.equal(context.TaliaVM.snapshot().state.presentation.cpu.chartLabel,'Past hour · 1-minute CPU averages',JSON.stringify(context.TaliaVM.snapshot()));
 vm.runInContext("TaliaVM.dispatch('cpuDay',{})",context);await new Promise(r=>setImmediate(r));assert.equal(context.TaliaVM.snapshot().state.presentation.cpu.chartLabel,'Past 24 hours · 5-minute CPU averages');
}
console.log('PASS: three host instances, isolated selectors/grants/subscriptions, shared responsive UI, stale/missing/down states');
