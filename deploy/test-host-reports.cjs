const fs=require('fs'), assert=require('assert');
const facts=eval('('+fs.readFileSync(__dirname+'/report-checklist-facts.js','utf8')+')');
const compose=eval('('+fs.readFileSync(__dirname+'/report-checklist-compose.js','utf8')+')');
const hosts=[['node-exporter','node-exporter:9100'],['node-exporter-vps','vps-eu'],['node-exporter-vps','vps-us']];
const services=[['lelloauth','lelloauth:8080'],['knot-resolver','knot-resolver:8453'],['pezzottify-server','192.168.1.34:9091'],['pezzottflix','192.168.1.34:9092'],['simple-ai','simple-ai:8080'],['simple-agents','simple-agents:9781']];
const row=(h,n,extra={})=>({metric:{job:h[0],instance:h[1],...extra},samples:[[1000,n]]});
const metric=(name,n,labels={})=>row(['',''],n,{__name__:name,...labels});
function context(){const steps={};for(const [id,n] of Object.entries({reachability:1,cpu:20,memory:30,disks:50,availability:1,systemd:10,failed:0}))steps[id]={status:'succeeded',value:{wire:{result:id==='failed'?[]:hosts.map(h=>row(h,n,id==='disks'?{mountpoint:'/'}:{}))}}};steps.disks.value.wire.result.push(row(hosts[0],90,{mountpoint:'/boot/efi'}),row(hosts[0],80,{mountpoint:'/mnt/external'}));for(const id of ['reachability','availability'])steps[id].value.wire.result.push(...services.map(s=>row(s,1)));
steps.signals={status:'succeeded',value:{wire:{result:[metric('talia_dns_servfail_ratio',0),metric('talia_music_error_ratio',0),metric('simpleai_up',1),metric('simple_agents_uncertain_effects',0),metric('simple_agents_exhausted_recovery',0)]}}};
const probes=[];for(const host of ['Homelab','VPS-EU','VPS-US'])for(const kind of ['dns','https'])probes.push(metric('talia_report_probe_success',1,{host,kind}),metric('talia_report_probe_checked_timestamp_seconds',1000,{host,kind}));steps.extras={status:'succeeded',value:{wire:{result:probes}}};for(const id of ['lellostore','crumbles'])steps[id]={status:'succeeded',value:{wire:{status:'healthy'}}};steps.reachability.value.wire.result.push({metric:{__name__:'up',job:'idle-manager',instance:'idle-manager:8090'},samples:[[1000,1]]});steps.analysis={status:'skipped',value:null};return {now:1000000,steps,decode:v=>v};}
let c=context(),f=facts(c);assert.equal(f.state,'Nominal');assert.equal(f.items.length,11);assert(!f.items.some(i=>['Observo','Backups','TLS certificates'].includes(i.name)));c.steps.facts={value:f};const healthy=compose(c).sections[0].text;assert.equal(healthy.split('\n').length,11);assert(healthy.split('\n').every(l=>/^✅ [^—]+$/.test(l)));assert(!healthy.includes('Assessment'));
c=context();c.steps.cpu.value.wire.result[1].samples[0][1]=94;f=facts(c);assert.equal(f.state,'Warning');assert.equal(f.items[0].state,'OK');assert(f.items[1].problems[0].text.includes('94.0%'));c.steps.facts={value:f};c.steps.analysis={status:'succeeded',value:{summary:JSON.stringify({assessments:[{id:'vps-eu',assessment:'Elevated CPU; cause is not established.'},{id:'lelloauth',assessment:'Do not display on healthy items.'}]})}};let rendered=compose(c).sections[0].text;assert(rendered.includes('Elevated CPU'));assert(!rendered.includes('Do not display'));c.steps.analysis.value.summary='not JSON';assert(compose(c).sections[0].text.includes('Assessment unavailable'));
c=context();c.steps.reachability.value.wire.result[2].samples[0][1]=0;f=facts(c);assert.equal(f.state,'Error');assert.equal(f.items[2].problems.length,1);
c=context();c.steps.failed.value.wire.result=[row(hosts[2],1,{name:'example.service'})];assert.equal(facts(c).items[2].state,'ERR');
c=context();c.steps.extras.value.wire.result=[];f=facts(c);assert.equal(f.items.filter(i=>i.state==='WARN').length,3);assert.equal(f.state,'Warning');
c=context();c.steps.lellostore.status='failed';assert.equal(facts(c).items.find(i=>i.id==='lellostore').state,'ERR');
c=context();c.steps.cpu.status='failed';assert(facts(c).items.slice(0,3).every(h=>h.state==='WARN'));
c=context();c.steps.signals.value.wire.result[0].samples[0][1]=0.02;assert.equal(facts(c).items.find(i=>i.id==='dns').state,'WARN');
console.log('11-item checklist: healthy output, thresholds, host grouping, missing coverage, severity precedence, per-item AI and malformed AI fallback passed.');

c=context();for(const r of c.steps.extras.value.wire.result) if(r.metric.__name__==='talia_report_probe_checked_timestamp_seconds' && r.metric.host==='VPS-EU') r.samples[0][1]=800;f=facts(c);assert.equal(f.items[1].state,'WARN');assert.equal(f.items[0].state,'OK');assert(f.items[1].problems.every(p=>p.text.includes('120 seconds')));
c=context();for(const r of c.steps.extras.value.wire.result) if(r.metric.__name__==='talia_report_probe_success' && r.metric.host==='VPS-US' && r.metric.kind==='https') r.samples[0][1]=0;f=facts(c);assert.equal(f.items[2].state,'WARN');assert.deepEqual(f.items[2].problems,[{severity:'WARN',text:'External HTTPS connection: probe failed.'}]);
console.log('Stale cached successes and real per-host HTTPS failures are distinguished.');

for (const [edit, expected] of [[()=>{}, "nominal"], [c=>c.steps.cpu.value.wire.result[1].samples[0][1]=94, "warning"], [c=>c.steps.reachability.value.wire.result[2].samples[0][1]=0, "error"]]) {
  const c=context(); edit(c); c.steps.facts={value:facts(c)}; assert.equal(compose(c).severity,expected);
}
for (const definition of JSON.parse(fs.readFileSync(__dirname+"/homelab-telegram-reports.json","utf8"))) {
  assert.equal(definition.compose,fs.readFileSync(__dirname+"/report-checklist-compose.js","utf8"),definition.id+" composer must match deployment source");
}
console.log("Composer severity and deployment template consistency passed.");
const boxo=n=>({metric:{__name__:'idle_manager_node_state',job:'idle-manager',instance:'idle-manager:8090',node:'boxo'},samples:[[1000,n]]});
const control=()=>({metric:{__name__:'up',job:'idle-manager',instance:'idle-manager:8090'},samples:[[1000,1]]});
const pezzottify=c=>facts(c).items.find(i=>i.id==='pezzottify');
const sleeping=c=>{const s=c.steps.reachability.value.wire.result.find(r=>r.metric.job==='pezzottify-server');s.samples[0][1]=0;c.steps.availability.value.wire.result=c.steps.availability.value.wire.result.filter(r=>r.metric.job!=='pezzottify-server');c.steps.signals.value.wire.result=c.steps.signals.value.wire.result.filter(r=>r.metric.__name__!=='talia_music_error_ratio');};
c=context();sleeping(c);c.steps.reachability.value.wire.result.push(boxo(5));assert.equal(pezzottify(c).state,'OK');assert.equal(facts(c).state,'Nominal');
c=context();sleeping(c);c.steps.reachability.value.wire.result.push(boxo(2));assert.equal(pezzottify(c).state,'ERR');
c=context();sleeping(c);assert.equal(pezzottify(c).state,'ERR','unknown node state still checks the service');
c=context();c.steps.availability.value.wire.result=c.steps.availability.value.wire.result.filter(r=>r.metric.job!=='pezzottify-server');c.steps.reachability.value.wire.result.push(boxo(2));assert.equal(pezzottify(c).state,'OK','no settled awake time is not a warning');
c=context();c.steps.reachability.value.wire.result.push(boxo(3));c.steps.availability.value.wire.result.find(r=>r.metric.job==='pezzottify-server').samples[0][1]=0.9;assert.equal(pezzottify(c).state,'WARN');
console.log("Sleeping Boxo is expected; awake or unknown Boxo still checks Pezzottify.");

for(const job of ['pezzottify-server','pezzottflix']) {
 const id=job==='pezzottflix'?'pezzottflix':'pezzottify';
 for(const state of [0,1,4,5]) {
  c=context();c.steps.reachability.value.wire.result.find(r=>r.metric.job===job).samples[0][1]=0;
  c.steps.reachability.value.wire.result.push(boxo(state));
  c.steps.availability.value.wire.result.find(r=>r.metric.job===job).samples[0][1]=0.1;
  assert.equal(facts(c).items.find(i=>i.id===id).state,'OK',id+' expected sleep/transition '+state);
 }
 for(const state of [2,3,6,null]) {
  c=context();c.steps.reachability.value.wire.result.find(r=>r.metric.job===job).samples[0][1]=0;
  if(state!==null)c.steps.reachability.value.wire.result.push(boxo(state));
  assert.equal(facts(c).items.find(i=>i.id===id).state,'ERR',id+' awake/error/unknown '+state);
 }
 c=context();c.steps.reachability.value.wire.result.find(r=>r.metric.job===job).samples[0][1]=0;
 c.steps.reachability.value.wire.result.push(boxo(5));
 c.steps.reachability.value.wire.result.find(r=>r.metric.job==='idle-manager').samples[0][1]=0;
 assert.equal(facts(c).items.find(i=>i.id===id).state,'ERR','failed controller must not hide '+id+' outage');
 c=context();c.steps.reachability.value.wire.result.find(r=>r.metric.job===job).metric.instance='retired:9092';
 c.steps.reachability.value.wire.result.push(boxo(2));
 assert.equal(facts(c).items.find(i=>i.id===id).state,'WARN','retired target must not count as current');
}
for(const definition of JSON.parse(fs.readFileSync(__dirname+'/homelab-telegram-reports.json','utf8'))) {
 assert.equal(definition.steps.find(s=>s.id==='facts').source,fs.readFileSync(__dirname+'/report-checklist-facts.js','utf8'));
 const query=eval('('+definition.steps.find(s=>s.id==='availability').request+')')({period:{end:1000000}}).query;
 assert(query.includes('job!~"pezzottify-server|pezzottflix"'));
 assert(query.includes('192[.]168[.]1[.]34:(9091|9092)'));
 assert(!query.includes('\\'));
}
console.log('Both Boxo services: intentional sleep, awake failures, unknown/failed controller, retired targets and availability query encoding passed.');
