import {DashboardHost} from './dashboard-host.js';
import {LIMITS} from './bridge-policy.js';
const check = (value, message) => { if (!value) throw Error(message); };
export async function runPolicySuite(bridge, lifecycle) {
  const host = new DashboardHost(), cases = [];
  try {
    const survivor = await host.create(bridge);
    await survivor.eval(lifecycle.start + ';void 0;');
    for (let i = 0; i < 100; i++) {
      if (await survivor.eval('report.done')) break;
      await new Promise(r => setTimeout(r, 5));
    }
    check(await survivor.eval('report.done'), 'survivor not ready');
    const survivorSub = [...host.subscriptions].find(([,g]) => g === survivor.generation)[0];
    const state = host.value;
    async function verify(victim, name, reason) {
      check(!victim.active && victim.reason.includes(reason), name + ': wrong rejection: ' + victim.reason);
      check(victim.pending.size === 0 && !host.guests.has(victim.generation), name + ': pending leak');
      check(![...host.subscriptions.values()].includes(victim.generation), name + ': subscription leak');
      check(![...host.timers.values()].includes(victim.generation), name + ': timer leak');
      check(!host.stalled.some(r => r.generation === victim.generation), name + ': stalled leak');
      check(host.value === state, name + ': engine mutated');
      host.deliver(survivor, {event:survivorSub, value:1});
      check(await survivor.eval('events.length') === cases.length + 1, name + ': survivor lost event');
      check(await survivor.eval('1+1') === 2, name + ': survivor stopped');
      check(host.stalled.some(r => r.generation === survivor.generation), name + ': survivor call lost');
      cases.push(name);
    }
    const attacks = [
      ['malformed JSON', `__send('{')`, 'bridge policy'],
      ['non-string wire', '__send({})', 'wire size/type'],
      ['oversized result', `'x'.repeat(70000)`, 'wire size/type'],
      ['wrong envelope', `__send('[]')`, 'request envelope'],
      ['extra routing field', `__send(JSON.stringify({id:1,op:'write',value:99,generation:1}))`, 'request envelope'],
      ['invalid write type', `__send(JSON.stringify({id:1,op:'write',value:'99'}))`, 'numeric value'],
      ['unknown capability', `__send(JSON.stringify({id:1,op:'fetch',value:null}))`, 'unknown capability'],
      ['unsafe ID', `__send(JSON.stringify({id:9007199254740992,op:'write',value:99}))`, 'request envelope'],
      ['replayed ID', `__send(JSON.stringify({id:1,op:'echo',value:null}));__send(JSON.stringify({id:1,op:'write',value:99}))`, 'request ID'],
      ['oversized UTF-8', `__send(JSON.stringify({id:1,op:'echo',value:'é'.repeat(17000)}))`, 'wire size/type'],
      ['deep JSON', `let v=null;for(let i=0;i<20;i++)v=[v];__send(JSON.stringify({id:1,op:'echo',value:v}))`, 'JSON complexity'],
      ['wide JSON', `__send(JSON.stringify({id:1,op:'echo',value:Array(2100).fill(0)}))`, 'JSON complexity'],
      ['request flood caught by guest', `for(let i=1;i<=1000;i++){try{__send(JSON.stringify({id:i,op:'echo',value:null}))}catch{}}`, 'in-flight request limit'],
      ['subscription budget', `for(let i=0;i<17;i++)engine.call('subscribe','value');`, 'subscription limit'],
      ['timer budget', `for(let i=0;i<17;i++)engine.call('delay','long');`, 'timer limit'],
      ['stalled budget', `for(let i=0;i<17;i++)engine.call('stall');`, 'stalled call limit'],
      ['cross-instance unsubscribe', `engine.call('unsubscribe',${JSON.stringify(survivorSub)})`, 'subscription ownership'],
    ];
    for (const [name, source, reason] of attacks) {
      const victim = await host.create(bridge);
      await victim.eval(source + (name === 'oversized result' ? '' : ';void 0;')).catch(() => {});
      // A reply can precede the parent-side violation; drain the same Worker port.
      if (victim.active) await victim.eval('0').catch(() => {});
      await verify(victim, name, reason);
    }
    // Parent checks cannot rely on validation having happened in the Worker.
    for (const raw of ['null', JSON.stringify({id:1,op:'write',value:{bad:true}}), 'x'.repeat(LIMITS.wireBytes + 1)]) {
      const victim = await host.create(bridge);
      await victim.eval("engine.call('subscribe','value');engine.call('delay','long');engine.call('stall');void 0;");
      host.request(victim, raw);
      await verify(victim, 'parent rejects ' + cases.length, 'bridge policy');
    }
    const oversizedCommand = await host.create(bridge);
    await oversizedCommand.eval('x'.repeat(LIMITS.commandBytes + 1)).catch(() => {});
    await verify(oversizedCommand, 'oversized command', 'wire size/type');
    const queued = await host.create(bridge);
    const commands = Array.from({length:LIMITS.commands + 1}, () => queued.eval('1'));
    const settled = await Promise.allSettled(commands);
    check(settled.every(p => p.status === 'rejected'), 'queued commands not rejected');
    await verify(queued, 'command queue budget', 'command queue limit');

    // Fill a recipient queue, then prove publish overload is charged to the producer.
    const publisher = await host.create(bridge);
    const backlog = Array.from({length:LIMITS.commands}, () => survivor.eval('1'));
    host.request(publisher, JSON.stringify({id:1,op:'publish',value:1}));
    await Promise.all(backlog);
    await verify(publisher, 'fanout isolation', 'publish fanout limit');

    const boundary = await host.create(bridge);
    const padding = LIMITS.wireBytes - JSON.stringify({id:1,op:'echo',value:''}).length;
    await boundary.eval(`globalThis.echoed=0;engine.call('echo','x'.repeat(${padding})).then(v=>echoed=v.length);void 0;`);
    for (let i = 0; i < 100 && await boundary.eval('echoed') !== padding; i++) await new Promise(r => setTimeout(r, 5));
    check(await boundary.eval('echoed') === padding, 'exact wire boundary rejected');
    for (const op of ['subscribe','delay','stall']) {
      const value = op === 'subscribe' ? 'value' : op === 'delay' ? 'long' : null;
      for (let i = 0; i < 16; i++) await boundary.eval(`engine.call(${JSON.stringify(op)},${JSON.stringify(value)});void 0;`);
    }
    check([...host.subscriptions.values()].filter(g => g === boundary.generation).length === LIMITS.subscriptions, 'subscription boundary');
    check([...host.timers.values()].filter(g => g === boundary.generation).length === LIMITS.timers, 'timer boundary');
    check(host.stalled.filter(r => r.generation === boundary.generation).length === LIMITS.stalled, 'stalled boundary');
    host.retire(boundary);
    const workers = [];
    while (host.guests.size < LIMITS.workers) workers.push(await host.create(bridge));
    await host.create(bridge).then(() => { throw Error('Worker budget not enforced'); }, e => check(e.message === 'Worker limit', 'wrong Worker failure'));
    check(await survivor.eval('1+1') === 2, 'Worker cap affected survivor');
    for (const worker of workers) host.retire(worker);
    host.retire(survivor);
    check(host.guests.size === 0 && host.subscriptions.size === 0 && host.timers.size === 0 && host.stalled.length === 0, 'final policy resource leak');
    return {passed:true, limits:LIMITS, cases, worker_budget:true, exact_boundaries:true, resources_empty:true};
  } finally { host.close(); }
}
