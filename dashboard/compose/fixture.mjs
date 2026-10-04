// Writes the Android device-test fixture: the homelab package compiled like the engine catalog
// compiler (functions via defineFunction, UI definitions inlined) and a TaliaValue-encoded sample.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import '../shared/ui.js';import '../../engine/shared/value.js';
import {changes,hosts} from '../../deploy/host-dashboards.mjs';
const doc=(kind,id)=>changes.find(c=>c.key.kind===kind&&c.key.id===id).document;
const dashboard=doc('dashboard','homelab');
const pkg={version:1,id:'homelab',revision:'catalog-fixture-homelab',ui:TaliaUI.compile(dashboard.ui),
 definitions:{'host-metric-card':TaliaUI.compileDefinition(doc('ui','host-metric-card').source),'host-layout':TaliaUI.compileDefinition(doc('ui','host-layout').source)},
 viewModel:`defineFunction("host-present",(${doc('function','host-present').source}));\n`+dashboard.view_model,params:dashboard.params,grants:dashboard.grants};
TaliaUI.validatePackage(pkg);
let seed=7;const rnd=()=>(seed=(seed*16807)%2147483647)/2147483647;
const series=(n,base,amp,spike)=>Array.from({length:n},(_,i)=>i<8?null:Math.max(0,Math.min(100,+(base+amp*Math.sin(i/25)+rnd()*amp+(i%97===50?spike:0)).toFixed(2))));
const now=Date.UTC(2026,9,4,16,30);
const value={updated:now,reachable:true,cpu:23.4,memory:61.2,memoryUsedBytes:19.6*2**30,memoryTotalBytes:32*2**30,memoryAvailableBytes:12.4*2**30,
 cpuHistory:series(289,15,8,70),cpuMinuteHistory:series(61,20,10,0),memoryHistory:series(289,58,3,0),
 disks:[{id:'root',mount:'/',device:'/dev/nvme0n1p2',free:41.3,availableBytes:193e9,totalBytes:467e9},{id:'data',mount:'/mnt/data',device:'/dev/sda1',free:6.2,availableBytes:248e9,totalBytes:4000e9},{id:'backup',mount:'/mnt/backup',device:'/dev/sdb1',free:72.8,availableBytes:1456e9,totalBytes:2000e9}],
 gitWorkspaces:{checked:now/1000-3600,lastClean:now/1000-86400*3,success:1,repositories:11,worktrees:14,dirty:['/home/lelloman/crumbles','/home/lelloman/homelab','/home/lelloman/pezzottify'],errors:[]}};
const fixture={catalog:{admin:false,dashboards:hosts.map(h=>({id:h.host})),defaultDashboard:'homelab'},package:pkg,
 values:[{id:'host-homelab',revision:1,quality:'good',value:TaliaValue.encode(value)}]};
const out=new URL('../../android/app/src/androidTest/assets/',import.meta.url);mkdirSync(out,{recursive:true});
writeFileSync(new URL('dashboard-fixture.json',out),JSON.stringify(fixture));
console.log('package bytes',JSON.stringify(pkg).length);
