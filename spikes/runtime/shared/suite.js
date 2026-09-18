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
  await engine.write(7);
  assert(await engine.read('value') === 7, 'engine side effect');
  report.done = true;
})().catch(e => { report.error = String(e.stack || e); report.done = true; });
