import test from 'node:test';import assert from 'node:assert/strict';import '../shared/value.js';import '../shared/client.js';
test('connection recovery reconciles before online; no mutation replay',async()=>{let now=0,inc='first',fail=false;const calls=[],states=[];const snapshot={revision:1,values:[{id:'value',revision:1,value:TaliaValue.encode(undefined),timestamp:12}]};const c=new TaliaConnection({client:'test',clock:()=>now,status:s=>states.push(s),send:async r=>{calls.push(r.op);if(fail)throw Error('offline');return {version:1,epoch:r.epoch,incarnation:inc,value:r.op==='status'?{status:'unknown'}:snapshot};}});await c.tick();assert.equal(c.state,'');assert.equal(c.sample().value,undefined);c.actions.set('a',{status:'unknown'});fail=true;await c.tick();assert.equal(c.state,'disconnected');now=1000;fail=false;inc='new';await c.tick();assert.equal(c.state,'back online');assert.equal(c.actions.get('a').status,'unknown');assert(calls.indexOf('status')>=0);assert(!calls.includes('write'));now=5000;await c.tick();assert.equal(c.state,'');assert.deepEqual(states,['','disconnected','connecting...','back online','']);});
test('late old-generation response cannot overwrite reconnect state',async()=>{let resolve;const c=new TaliaConnection({client:'test',send:()=>new Promise(r=>resolve=r)});const p=c.call('snapshot').catch(e=>e.message);c.epoch++;resolve({version:1,epoch:1,incarnation:'old',value:{}});assert.equal(await p,'obsolete request');});
test('named resources reconcile independently and manual runs are never replayed',async()=>{
 let now=0,inc='one',offline=false;const requests=[];
 const values=['cpu','disk'].map((id,n)=>({id,revision:1,value:TaliaValue.encode(n?NaN:42),hasValue:true,timestamp:12}));
 const snapshot={revision:1,values};
 const c=new TaliaConnection({client:'named',clock:()=>now,send:async r=>{requests.push([r.op,r.args]);if(offline)throw Error('offline');let value=snapshot;if(r.op==='run')value={actionId:r.args.actionId,status:'running',runId:'run-1'};if(r.op==='status')value={actionId:r.args.actionId,status:'unknown',runId:'run-1'};if(r.op==='read')value=values.find(v=>v.id===r.args.id);return {version:1,epoch:r.epoch,incarnation:inc,value};}});
 await c.setResources(['cpu','disk']);await c.setActive(true);await c.tick();assert.equal(c.sample('cpu').value,42);assert(Number.isNaN((await c.read('disk')).value));assert.deepEqual([...c.subscribed],['cpu','disk']);
 await c.run('probe','request-1');offline=true;await c.tick();now=1000;offline=false;inc='two';await c.tick();assert.equal(c.actions.get('request-1').status,'unknown');assert.equal(requests.filter(([op])=>op==='run').length,1);assert.deepEqual([...c.subscribed],['cpu','disk']);
 await c.setResources(['disk']);assert.deepEqual([...c.subscribed],['disk']);await c.setActive(false);assert.equal(c.subscribed.size,0);
});

test('host switching drops late replies without reporting an outage',async()=>{
 const states=[];let delayed,release,started,host='homelab';const polling=new Promise(resolve=>started=resolve);
 const c=new TaliaConnection({client:'switch',status:s=>states.push(s),send:async r=>{
  const snapshot={revision:1,values:[{id:host,revision:1,value:TaliaValue.encode(host)}]};
  if(delayed&&r.op==='poll'){delayed=false;await new Promise(resolve=>{release=resolve;started();});}
  return {version:1,epoch:r.epoch,incarnation:'server',value:snapshot};
 }});
 await c.setResources(['homelab']);await c.setActive(true);await c.tick();states.length=0;
 delayed=true;const old=c.tick();await polling;assert.equal(typeof release,'function');
 host='vps-eu';c.replaceContext();await c.setResources(['vps-eu']);release();await old;
 assert.equal(c.values.size,0);assert.equal(c.state,'');
 await c.tick();assert.equal(c.sample('vps-eu').value,'vps-eu');assert.deepEqual([...c.subscribed],['vps-eu']);
 assert.deepEqual(states,[]);
 // An idle switch is also quiet.
 host='homelab';c.replaceContext();await c.setResources(['homelab']);await c.tick();
 assert.equal(c.sample('homelab').value,'homelab');assert.deepEqual(states,[]);
});

test('a genuine failure while switching hosts still reports disconnect and recovery',async()=>{
 let now=0,fail=false;const states=[];
 const c=new TaliaConnection({client:'switch-outage',clock:()=>now,status:s=>states.push(s),send:async r=>{
  if(fail)throw Error('network failed');
  return {version:1,epoch:r.epoch,incarnation:'server',value:{revision:1,values:[]}};
 }});
 await c.tick();states.length=0;c.replaceContext();fail=true;await c.tick();
 assert.equal(c.state,'disconnected');now=1000;fail=false;await c.tick();
 assert.deepEqual(states,['disconnected','connecting...','back online']);
});
