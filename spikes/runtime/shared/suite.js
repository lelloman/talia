(async () => {
  assert(typeof fetch === 'undefined' && typeof document === 'undefined' &&
    typeof require === 'undefined' && typeof process === 'undefined' &&
    typeof viewDefinition === 'undefined', 'no ambient platform or renderer APIs');
  assert(vm.state.value === 1 && vm.action() === 2, 'baseline ViewModel restored');
  const payload = { name: 'Talìa', values: [null, true, 1.25], nested: { ok: true } };
  const echoed = await engine.call('echo', payload);
  assert(echoed.name === payload.name && echoed.values[0] === null && echoed.values[1] === true && echoed.values[2] === 1.25 && echoed.nested.ok, 'JSON roundtrip');
  let invalid = 0;
  for (const value of [NaN, Infinity, undefined, 1n, () => {}, new Date()]) {
    try { await engine.call('echo', { value }); } catch { invalid++; }
  }
  assert(invalid === 6, 'unsupported wire values rejected');
  let rejected = false;
  try { await engine.call('fail'); } catch (e) { rejected = e.message === 'host failure'; }
  assert(rejected, 'host rejection');
  const gate = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return {promise, resolve}; };
  async function rejects(promise, message) {
    try { await promise; return false; } catch (error) { return error.message === message; }
  }
  const baseline = new ExecutionScheduler(), waiting=gate(), release=gate();
  baseline.define('value', {initial:0, readMode:'independent', get:async ctx => {
    const before=ctx.snapshot(); waiting.resolve(); await release.promise;
    ctx.commit(before,1); return 1;
  }});
  const pendingRead=baseline.read('value');
  const stale=rejects(pendingRead.promise,'stale operation');
  await waiting.promise;
  await baseline.start('value',ctx=>ctx.commit(ctx.snapshot(),2)).promise;
  assert(await baseline.start('value',ctx=>ctx.snapshot().value).promise===2,
    'same-instance setter progresses during awaited getter');
  release.resolve();
  assert(await stale, 'stale getter cannot overwrite setter');
  await baseline.start('value',()=>{throw Error('guest failure');}).promise.catch(()=>{});
  let falsyRejected=0;
  for (const error of [null,undefined,0,false,'']) {
    await baseline.start('value',()=>{throw error;}).promise.then(
      ()=>{}, caught=>{if(caught===error) falsyRejected++;});
  }
  assert(falsyRejected===5 && await baseline.start('value',()=>3).promise===3,
    'operation progresses after rejection including falsy errors');
  let loads=0;
  baseline.define('cache',{readMode:'shared',get:()=>engine.call('echo',++loads)});
  assert((await Promise.all([baseline.read('cache').promise,baseline.read('cache').promise])).join(',')==='1,1' && loads===1,
    'shared refresh executes one getter');
  let events = 0;
  const unsubscribe = await engine.subscribe('value', value => { events += value; });
  await engine.call('publish', 3);
  assert(events === 3, 'subscription delivers host event');
  await unsubscribe();
  await engine.call('publish', 5);
  assert(events === 3, 'unsubscribe stops host events');
  const stalled = engine.call('stall').then(() => false, e => e.message === 'cancelled');
  engine.cancelPending();
  assert(await stalled, 'pending host operation cancellation');
  await engine.call('late');
  assert(true, 'late and duplicate replies ignored');
  const execution=[], scheduler=new ExecutionScheduler();
  const verify=(value,name)=>{if(!value) throw Error(name); execution.push(name);};
  const define=(key,get=ctx=>ctx.snapshot().value,readMode='independent',initial=0)=>
    scheduler.define(key,{initial,readMode,get});
  define('a',ctx=>ctx.read('a'));
  verify(await rejects(scheduler.read('a').promise,'dependency cycle'),'direct dependency cycle rejected');
  define('a',ctx=>ctx.read('b')); define('b',ctx=>ctx.read('a'));
  verify(await rejects(scheduler.read('a').promise,'dependency cycle'),'indirect dependency cycle rejected');
  const rootsGo=gate(), rootsReady=gate(); let rootsStarted=0;
  const rootGetter=other=>async ctx=>{
    if(++rootsStarted===2) rootsReady.resolve();
    await rootsGo.promise; return ctx.read(other);
  };
  define('a',rootGetter('b'),'shared'); define('b',rootGetter('a'),'shared');
  const rootA=rejects(scheduler.read('a').promise,'dependency cycle');
  const rootB=rejects(scheduler.read('b').promise,'dependency cycle');
  await rootsReady.promise; rootsGo.resolve();
  verify(await rootA && await rootB,'concurrent shared roots reject dependency cycle');
  let diamondLoads=0;
  define('leaf',async()=>{await engine.call('delay'); return ++diamondLoads;},'shared');
  define('left',ctx=>ctx.read('leaf')); define('right',ctx=>ctx.read('leaf'));
  define('root',ctx=>Promise.all([ctx.read('left'),ctx.read('right')]));
  verify((await scheduler.read('root').promise).join(',')==='1,1' && diamondLoads===1,
    'acyclic diamond shares dependency');

  const overlap=gate(), bothReady=gate(); let count=0;
  define('independent',async()=>{if(++count===2) bothReady.resolve(); await overlap.promise; return count;});
  const i1=scheduler.read('independent'), i2=scheduler.read('independent');
  await bothReady.promise;
  verify(count===2,'same-instance independent getters overlap');
  overlap.resolve(); await Promise.all([i1.promise,i2.promise]);

  define('state');
  const writeReady=gate(), writeGo=gate();
  const oldWrite=scheduler.start('state',async ctx=>{
    const snapshot=ctx.snapshot(); writeReady.resolve(); await writeGo.promise; ctx.commit(snapshot,99);
  });
  const oldRejected=rejects(oldWrite.promise,'stale operation'); await writeReady.promise;
  await scheduler.start('state',ctx=>ctx.commit(ctx.snapshot(),5)).promise;
  writeGo.resolve();
  verify(await oldRejected && await scheduler.read('state').promise===5,'competing stale commit rejected');
  verify(await scheduler.start('state',ctx=>{
    const snapshot=ctx.snapshot(); ctx.commit(snapshot,6);
    try {ctx.commit(snapshot,7);return false;} catch(e){return e.message==='stale snapshot';}
  }).promise,'snapshot cannot be reused after commit');
  verify(await scheduler.start('state',ctx=>{
    try {ctx.commit(ctx.snapshot(),Promise.resolve(8));return false;}
    catch(e){return e.message==='state must be JSON';}
  }).promise,'async state update rejected');
  verify(await scheduler.start('state',ctx=>{
    const candidate={nested:{n:1}}; ctx.commit(ctx.snapshot(),candidate); candidate.nested.n=9;
    const snapshot=ctx.snapshot(); snapshot.value.nested.n=8;
    return ctx.snapshot().value.nested.n===1;
  }).promise,'state snapshots and commits do not alias caller objects');

  const staleReady=gate(), staleGo=gate();
  define('refresh',async()=>{staleReady.resolve(); await staleGo.promise; return 10;},'shared');
  const staleResult=rejects(scheduler.read('refresh').promise,'stale operation');
  await staleReady.promise; scheduler.invalidate('refresh'); staleGo.resolve();
  verify(await staleResult,'invalidation fences suspended getter publication');
  const setterReady=gate(), setterGo=gate();
  define('set-shared',async()=>{setterReady.resolve();await setterGo.promise;return 1;},'shared');
  const setterOld=rejects(scheduler.read('set-shared').promise,'stale operation');
  await setterReady.promise;
  await scheduler.start('set-shared',ctx=>ctx.commit(ctx.snapshot(),42)).promise;
  verify(await scheduler.start('set-shared',ctx=>ctx.snapshot().value).promise===42,
    'setter progresses during shared refresh');
  setterGo.resolve();verify(await setterOld,'setter fences old shared refresh result');
  define('publication',()=>1);
  const publication=rejects(scheduler.read('publication').promise,'stale operation');
  await Promise.resolve(); await Promise.resolve();
  scheduler.invalidate('publication');
  verify(await publication,'publication rechecks revision after Promise adoption');
  const configReady=gate(), configGo=gate();
  define('config',async()=>{configReady.resolve();await configGo.promise;return 1;},'shared');
  const configResult=rejects(scheduler.read('config').promise,'stale operation');
  await configReady.promise; define('config',()=>2,'shared');
  verify(await scheduler.read('config').promise===2,'replacement definition runs during old getter');
  configGo.resolve(); verify(await configResult,'definition replacement fences old result');

  let skipped=false;
  const unstarted=scheduler.start('state',()=>{skipped=true;});
  const unstartedRejected=rejects(unstarted.promise,'cancelled'); unstarted.cancel();
  verify(await unstartedRejected && !skipped,'cancelled work skipped before start');
  const cancelReady=gate(), cancelGo=gate(), cancelFinished=gate(); let forbidden=false;
  const running=scheduler.start('state',async ctx=>{
    const before=ctx.snapshot(); cancelReady.resolve(); await cancelGo.promise;
    try {ctx.commit(before,100); forbidden=true;} finally {cancelFinished.resolve();}
  });
  const cancelled=rejects(running.promise,'cancelled'); await cancelReady.promise; running.cancel();
  verify(await cancelled && await scheduler.start('state',()=>8).promise===8,
    'cancelled suspended work does not block same instance');
  cancelGo.resolve(); await cancelFinished.promise;
  verify(!forbidden,'cancelled running commit rejected');

  let sharedLoads=0; const sharedReady=gate(), sharedGo=gate();
  define('shared',async()=>{sharedLoads++;sharedReady.resolve();await sharedGo.promise;return 12;},'shared');
  const reader1=scheduler.read('shared'), reader2=scheduler.read('shared');
  const reader1Rejected=rejects(reader1.promise,'cancelled');
  await sharedReady.promise; reader1.cancel(); sharedGo.resolve();
  verify(await reader1Rejected && await reader2.promise===12 && sharedLoads===1,
    'one cancelled reader preserves shared refresh for others');
  const abandonedReady=gate(), abandonedGo=gate(), abandonedFinished=gate(); let refreshes=0, lateEffect=false;
  define('abandoned',async ctx=>{
    if(++refreshes>1) return 22;
    abandonedReady.resolve();await abandonedGo.promise;
    try {ctx.effect(()=>{lateEffect=true;});return 21;} finally {abandonedFinished.resolve();}
  },'shared');
  const abandoned=scheduler.read('abandoned'); const abandonedRejected=rejects(abandoned.promise,'cancelled');
  await abandonedReady.promise; abandoned.cancel();
  verify(await abandonedRejected && await scheduler.read('abandoned').promise===22,
    'last reader cancellation permits immediate replacement refresh');
  abandonedGo.resolve();await abandonedFinished.promise;
  verify(!lateEffect,'cancelled continuation cannot dispatch new effect');

  const childReady=gate(), childGo=gate(); let childLoads=0;
  define('child',async()=>{childLoads++;childReady.resolve();await childGo.promise;return 31;},'shared');
  define('parent',ctx=>ctx.read('child'));
  const parent=scheduler.read('parent'), outsider=scheduler.read('child');
  const parentRejected=rejects(parent.promise,'cancelled');
  await childReady.promise;parent.cancel();childGo.resolve();
  verify(await parentRejected && await outsider.promise===31 && childLoads===1,
    'parent cancellation preserves dependency owned by another reader');

  const ownedReady=gate(), ownedGo=gate(), ownedDone=gate();let childEffect=false;
  define('owned',async ctx=>{ownedReady.resolve();await ownedGo.promise;
    try {ctx.effect(()=>{childEffect=true;});} finally {ownedDone.resolve();}},'shared');
  define('owner',ctx=>ctx.read('owned'));
  const owner=scheduler.read('owner'), ownerRejected=rejects(owner.promise,'cancelled');
  await ownedReady.promise;owner.cancel();ownedGo.resolve();await ownedDone.promise;
  verify(await ownerRejected && !childEffect,'sole parent cancellation cancels dependency');
  let retainedContext; define('retained',ctx=>{retainedContext=ctx;return 1;});
  await scheduler.read('retained').promise;
  let lateRejected=false;
  try {retainedContext.effect(()=>{});}catch(e){lateRejected=e.message==='operation finished';}
  verify(lateRejected,'settled operation cannot dispatch detached effects');
  const effectReady=gate(), effectGo=gate(); define('effect');
  const effect=scheduler.start('effect',async ctx=>{
    ctx.commit(ctx.snapshot(),4);
    await ctx.effect(()=>engine.write(11));effectReady.resolve();await effectGo.promise;
  });
  const effectRejected=rejects(effect.promise,'cancelled');
  await effectReady.promise;effect.cancel();effectGo.resolve();
  verify(await effectRejected && await engine.read('value')===11 && await scheduler.read('effect').promise===4,
    'cancellation preserves earlier state and dispatched engine effects');
  // Drain native/browser Promise jobs via the host, after all test gates are released.
  await engine.call('delay');
  verify(Object.values(scheduler.stats()).every(n=>n===0),'execution tasks readers and wait edges released');
  const disposalReady=gate(), disposalGo=gate(), disposalDone=gate();
  define('dispose',async ctx=>{disposalReady.resolve();await disposalGo.promise;
    try{ctx.check();}finally{disposalDone.resolve();}});
  const disposed=rejects(scheduler.read('dispose').promise,'cancelled');
  await disposalReady.promise;scheduler.dispose();disposalGo.resolve();await disposalDone.promise;
  await engine.call('delay');
  verify(await disposed && scheduler.stats().tasks===0,'disposal cancels pending work and releases tasks');
  report.execution=execution;
  await engine.write(7);
  assert(await engine.read('value') === 7, 'engine side effect');
  report.done = true;
})().catch(e => { report.error = String(e.stack || e); report.done = true; });
