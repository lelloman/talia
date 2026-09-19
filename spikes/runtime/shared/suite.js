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
  const order = [], state = { value: 0 };
  const a = serial('a', async () => {
    order.push('a:start');
    await engine.call('delay');
    state.value = 1;
    order.push('a:end');
  });
  const b = serial('a', async () => {
    assert(state.value === 1, 'setter observes completed getter state');
    order.push('a:set'); state.value = 2;
  });
  const c = serial('other', async () => { order.push('other'); });
  await Promise.all([a,b,c]);
  assert(order.join(',') === 'a:start,other,a:end,a:set', 'per-instance serialization across await');
  await serial('a', async () => { throw Error('guest failure'); }).catch(() => {});
  await serial('a', async () => assert(state.value === 2, 'queue continues after rejection'));
  let loads = 0;
  const cache = {};
  async function get() {
    return serial('cache', async () => {
      if (cache.expires === undefined || clock.now() >= cache.expires) {
        cache.value = await engine.call('echo', ++loads); cache.expires = clock.now() + 100;
      }
      return cache.value;
    });
  }
  assert((await Promise.all([get(), get()])).join(',') === '1,1' && loads === 1, 'concurrent cache miss loads once');
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
  // Execution-policy candidate: separate report, preserving the original 14 checks.
  const execution = [], scheduler = new ExecutionScheduler();
  function verify(value, name) { if (!value) throw Error(name); execution.push(name); }
  async function rejects(promise, message) {
    try { await promise; return false; } catch (error) { return error.message === message; }
  }
  const gate = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return {promise, resolve}; };
  verify(await rejects(scheduler.start('a', ctx => ctx.read('a', () => 0)).promise, 'dependency cycle'), 'direct dependency cycle rejected');
  verify(await rejects(scheduler.start('a', a => a.read('b', b => b.read('a', () => 0))).promise, 'dependency cycle'), 'indirect dependency cycle rejected');
  const readyA=gate(), readyB=gate(), releaseA=gate(), releaseB=gate();
  const rootA=scheduler.start('a', async a => { readyA.resolve(); await releaseA.promise; return a.read('b', () => 1); });
  const rootB=scheduler.start('b', async b => { readyB.resolve(); await releaseB.promise; return b.read('a', () => 2); });
  const settledA=rootA.promise.then(() => 'ok', e => e.message);
  const settledB=rootB.promise.then(() => 'ok', e => e.message);
  await Promise.all([readyA.promise, readyB.promise]); releaseA.resolve(); releaseB.resolve();
  const roots=await Promise.all([settledA,settledB]);
  verify(roots.includes('dependency cycle') && roots.includes('ok'), 'concurrent root cycle rejected without deadlock');
  const diamond=await scheduler.start('root', ctx => Promise.all([
    ctx.read('left', left => left.read('shared', () => 3)),
    ctx.read('right', right => right.read('shared', () => 4)),
  ])).promise;
  verify(diamond.join(',') === '3,4', 'acyclic shared dependency accepted');
  verify(await scheduler.start('a', () => 9).promise === 9, 'queue recovers after cycle rejection');

  const started=gate(), finish=gate(); let committed=0, setterRan=false, queuedRan=false, forbiddenCommit=false;
  const running=scheduler.start('value', async ctx => {
    started.resolve(); await finish.promise; ctx.commit(() => { forbiddenCommit=true; committed=99; });
  });
  const runningRejected=rejects(running.promise,'cancelled');
  await started.promise;
  const queued=scheduler.start('value', () => { queuedRan=true; });
  const queuedRejected=rejects(queued.promise,'cancelled'); queued.cancel(); running.cancel();
  const setter=scheduler.start('value', ctx => ctx.commit(() => { setterRan=true; committed=5; }));
  await engine.call('delay');
  verify(!setterRan, 'cancelled running work retains serialization slot');
  verify(await scheduler.start('independent', () => 8).promise === 8, 'independent instance progresses during cancellation');
  finish.resolve();
  verify(await runningRejected && !forbiddenCommit, 'cancelled running commit rejected');
  verify(await queuedRejected && !queuedRan, 'cancelled queued callback skipped');
  await setter.promise;
  verify(committed===5 && setterRan, 'queue recovers after cancellation');

  const childReady=gate(), childFinish=gate(); let childCommitted=false;
  const parent=scheduler.start('parent', ctx => ctx.read('child', async child => {
    childReady.resolve(); await childFinish.promise;
    child.commit(() => { childCommitted=true; });
  }));
  const parentRejected=rejects(parent.promise,'cancelled');
  await childReady.promise; parent.cancel(); childFinish.resolve();
  verify(await parentRejected && !childCommitted, 'cancellation propagates to dependency work');
  const effectStarted=gate(), effectFinish=gate();
  const effect=scheduler.start('effect', async ctx => {
    await ctx.effect(() => engine.write(11)); effectStarted.resolve();
    await effectFinish.promise; ctx.check();
  });
  const effectRejected=rejects(effect.promise,'cancelled');
  await effectStarted.promise; effect.cancel(); effectFinish.resolve();
  verify(await effectRejected && await engine.read('value')===11, 'cancellation preserves dispatched engine effect');
  verify(scheduler.tasks.size===0 && scheduler.tails.size===0, 'execution graph and queues released');
  report.execution=execution;
  await engine.write(7);
  assert(await engine.read('value') === 7, 'engine side effect');
  report.done = true;
})().catch(e => { report.error = String(e.stack || e); report.done = true; });
