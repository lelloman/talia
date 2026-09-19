import {runExecutionBoundarySuite} from './execution-suite.js';
import {runCapabilitySuite} from './capability-suite.js';
import {runPolicySuite} from './policy-suite.js';
import {DashboardHost} from './dashboard-host.js';
import {memoryCases} from '../memory-probe.mjs';
const check = (condition, message) => { if (!condition) throw Error(message); };
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
export async function runDisposableSuite() {
  const [bridge, suite, lifecycle] = await Promise.all([
    fetch('/shared/bridge.js').then(r => r.text()),
    fetch('/shared/suite.js').then(r => r.text()),
    fetch('/shared/lifecycle.json').then(r => r.json()),
  ]);
  const host = new DashboardHost(), start = performance.now();
  async function done(guest) {
    for (let i = 0; i < 200; i++) {
      const report = JSON.parse(await guest.eval('JSON.stringify(report)'));
      if (report.done) { check(!report.error, report.error); return report; }
      await sleep(5);
    }
    throw Error('fixture completion deadline');
  }
  const subscription = guest => [...host.subscriptions].find(([,g]) => g === guest.generation)?.[0];
  const response = guest => host.stalled.find(r => r.generation === guest.generation);
  function released(guest) {
    check(!host.guests.has(guest.generation) && !guest.active, 'Worker not retired');
    check(!subscription(guest) && !response(guest), 'host resources leaked');
    check(![...host.timers.values()].includes(guest.generation), 'host timer leaked');
    check(guest.pending.size === 0, 'command promises leaked');
  }
  async function active() {
    const guest = await host.create(bridge);
    await guest.eval(lifecycle.start + ';void 0;'); await done(guest);
    await guest.eval("engine.call('delay','long');void 0;");
    check(subscription(guest) && response(guest), 'missing live resources');
    check([...host.timers.values()].includes(guest.generation), 'missing live timer');
    return guest;
  }
  try {
    let checks, execution;
    for (let i = 0; i < 20; i++) {
      const guest = await host.create(bridge);
      await guest.eval(suite + ';void 0;'); const report = await done(guest); checks = report.checks; execution = report.execution;
      await guest.eval('vm.state.value=9;vm.action=()=>10;void 0;');
      check(await guest.eval('JSON.stringify([vm.state.value,vm.action()])') === '[9,10]', 'live patch failed');
      check(guest.memory_bytes === 16777216, 'module cap changed');
      host.retire(guest); released(guest);
    }
    const survivor = await active();
    const old = await active(), oldResponse = response(old), oldSubscription = subscription(old);
    host.retire(old); released(old);
    const current = await active();
    check(response(current).id === oldResponse.id, 'request IDs did not collide');
    check(!host.deliver(current, {id:oldResponse.id,value:'stale'}, old.generation), 'stale reply accepted');
    check(!host.deliver(current, {event:subscription(current),value:99}, old.generation), 'stale event accepted');
    check(!host.deliver(current, {event:oldSubscription,value:99}, old.generation), 'old subscription accepted');
    check(!host.deliver(old, {id:oldResponse.id,value:'stale'}), 'retired Worker accepted reply');
    check(await current.eval('JSON.stringify([reply,events.length])') === '[null,0]', 'stale state mutation');
    host.deliver(current, {id:response(current).id,value:'new'});
    host.deliver(current, {event:subscription(current),value:3});
    await current.eval(lifecycle.verify);
    check(await current.eval('JSON.stringify([vm.state.value,vm.action()])') === '[1,2]', 'baseline not restored');
    check(host.value === 7, 'engine effect lost');
    host.retire(current); released(current);

    let event = 0;
    async function survivorWorks() {
      host.deliver(survivor, {event:subscription(survivor),value:++event});
      check(await survivor.eval('events.length') === event, 'survivor event lost');
      check(await survivor.eval('1+1') === 2, 'survivor unresponsive');
      check(response(survivor), 'survivor request retired');
      check([...host.timers.values()].includes(survivor.generation), 'survivor timer retired');
    }
    const failures = [];
    for (const [name, source] of Object.entries(memoryCases)) {
      const victim = await active();
      const error = await victim.eval(source + ';void 0;').then(() => null, e => String(e));
      check(error && victim.reason.startsWith('guest failure:'), 'pressure did not fail in guest');
      check(/out of memory|Error: null/.test(error), 'unexpected allocation failure');
      check(victim.memory_bytes === 16777216, 'failed module exceeded cap');
      released(victim); await survivorWorks();
      const replacement = await host.create(bridge);
      check(await replacement.eval('JSON.stringify([vm.state.value,vm.action()])') === '[1,2]', 'replacement not clean');
      host.retire(replacement); released(replacement);
      failures.push({name, retired:true, memory_bytes:victim.memory_bytes, error,
        resources_released:true, survivor_works:true, replacement_works:true});
    }
    const runaway = await active(), interruptStart = performance.now();
    const interruptError = await runaway.command('interrupt').then(() => null, e => String(e));
    const interruption_ms = performance.now() - interruptStart;
    check(/interrupt/i.test(interruptError), 'loop was not interrupted');
    released(runaway); await survivorWorks();

    // The Worker acknowledges entering a host-side hang; its JS event loop cannot clean up.
    const hung = await active();
    const hungResult = hung.command('hang', null, 250).then(() => null, e => String(e));
    for (let i = 0; i < 40 && !hung.hanging && hung.active; i++) await sleep(5);
    check(hung.hanging && hung.active, 'Worker did not enter hang');
    await survivorWorks(); // Prove progress while the other Worker is still hung.
    check(hung.active, 'watchdog fired before concurrent progress was observed');
    check(await hungResult === 'Error: watchdog', 'watchdog did not reject pending command');
    released(hung); await survivorWorks();
    const recovered = await host.create(bridge);
    await recovered.eval(suite + ';void 0;');
    check(JSON.stringify((await done(recovered)).checks) === JSON.stringify(checks), 'full suite failed after recovery');
    host.retire(recovered); host.retire(survivor);
    check(host.guests.size === 0 && host.subscriptions.size === 0 && host.stalled.length === 0 && host.timers.size === 0, 'final resource leak');
    const policy = await runPolicySuite(bridge, lifecycle);
    return {passed:true, capabilities:await runCapabilitySuite(bridge), protected_execution:await runExecutionBoundarySuite(bridge), policy, cycles:20, checks, execution, module_limit_bytes:16777216,
      lifecycle:{forced_reload_cleanup:true, stale_response_rejected:true, stale_event_rejected:true,
        colliding_request_ids:true, other_instance_survives:true},
      failures, interruption_ms, watchdog_cleanup:true, progress_during_hang:true,
      full_suite_after_recovery:true, host_resources_empty:true, elapsed_ms:performance.now()-start};
  } finally { host.close(); }
}
