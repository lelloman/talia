import { newQuickJSWASMModule as oldModule, newVariant as oldVariant, RELEASE_SYNC as OLD } from 'quickjs-emscripten';
import { newQuickJSWASMModule as newModule, newVariant, RELEASE_SYNC as NEW } from 'quickjs-new';
import NG from '@jitl/quickjs-ng-wasmfile-release-sync';
import { probeMemory } from '../memory-probe.mjs';
import { probeCappedMemory } from '../capped-memory.mjs';

self.onmessage = async () => {
  try {
    const [bridge, suite, lifecycleScripts] = await Promise.all([
      fetch('/shared/bridge.js').then(r => r.text()),
      fetch('/shared/suite.js').then(r => r.text()),
      fetch('/shared/lifecycle.json').then(r => r.json()),
    ]);
    const Q = await newModule(newVariant(NG, { wasmLocation: '/web/dist/ng.wasm' }));
    const host = { value: 0, subscriptions: new Map(), next: 0, stalled: [], generation: 0 };
    const start = performance.now();

    function guest() {
      const generation = ++host.generation;
      const rt = Q.newRuntime();
      rt.setMemoryLimit(16 * 1024 * 1024);
      rt.setMaxStackSize(512 * 1024);
      const until = performance.now() + 3000;
      rt.setInterruptHandler(() => performance.now() > until);
      const vm = rt.newContext(), out = [];
      let active = true;
      const send = vm.newFunction('__send', raw => {
        if (!active) throw Error('retired runtime');
        out.push(JSON.parse(vm.getString(raw)));
      });
      vm.setProp(vm.global, '__send', send); send.dispose();
      function evaluate(code) {
        const result = vm.evalCode(code);
        if (result.error) {
          const error = vm.dump(result.error); result.error.dispose();
          throw Error(JSON.stringify(error));
        }
        const value = vm.dump(result.value); result.value.dispose();
        return value;
      }
      // Generation is supplied by trusted routing, not by guest payloads.
      function deliver(message, origin = generation) {
        if (!active || origin !== generation) return false;
        evaluate(`__receive(${JSON.stringify(JSON.stringify(message))})`);
        return true;
      }
      evaluate(bridge);
      return { rt, vm, out, generation, evaluate, deliver, dispose() {
        active = false;
        for (const [id, owner] of host.subscriptions) {
          if (owner === generation) host.subscriptions.delete(id);
        }
        host.stalled = host.stalled.filter(r => r.generation !== generation);
        out.length = 0;
        vm.dispose(); rt.dispose();
      }};
    }

    async function drive(g) {
      const until = performance.now() + 4000;
      while (performance.now() < until) {
        const jobs = g.rt.executePendingJobs(10000);
        if (jobs.error) {
          const error = g.vm.dump(jobs.error); jobs.error.dispose();
          throw Error(JSON.stringify(error));
        }
        const request = g.out.shift();
        if (!request) {
          const report = JSON.parse(g.evaluate('JSON.stringify(report)'));
          if (report.done) { if (report.error) throw Error(report.error); return report; }
          await new Promise(resolve => setTimeout(resolve, 1)); continue;
        }
        const { id, op, value } = request;
        let result = null;
        switch (op) {
          case 'echo': result = value; break;
          case 'delay': await new Promise(resolve => setTimeout(resolve, 5)); break;
          case 'read': result = host.value; break;
          case 'write': result = host.value = value; break;
          case 'fail': g.deliver({ id, error: 'host failure' }); continue;
          case 'subscribe': result = 's' + (++host.next); host.subscriptions.set(result, g.generation); break;
          case 'unsubscribe':
            if (host.subscriptions.get(value) === g.generation) host.subscriptions.delete(value);
            break;
          case 'publish':
            for (const [subscription, generation] of host.subscriptions) {
              g.deliver({ event: subscription, value }, generation);
            }
            break;
          case 'stall': host.stalled.push({ generation: g.generation, id }); continue;
          case 'late':
            for (const old of host.stalled.splice(0)) g.deliver({ id: old.id, value: 999 }, old.generation);
            break;
          default: throw Error('unknown capability');
        }
        g.deliver({ id, value: result });
        if (op === 'late') g.deliver({ id, value: result });
      }
      throw Error('host deadline exceeded');
    }

    function check(condition, message) { if (!condition) throw Error(message); }
    async function checkLifecycle() {
      const survivor = guest();
      survivor.evaluate(lifecycleScripts.start + ';void 0;'); await drive(survivor);
      const old = guest();
      old.evaluate(lifecycleScripts.start + ';void 0;'); await drive(old);
      const oldReply = host.stalled.find(r => r.generation === old.generation);
      const oldSubscription = [...host.subscriptions].find(([,g]) => g === old.generation)[0];
      check(host.subscriptions.size === 2, 'active subscriptions missing');
      old.dispose(); // Intentionally skip guest unsubscribe and cancellation.
      check(host.subscriptions.size === 1, 'retired subscription not released');
      check(!host.stalled.some(r => r.generation === old.generation), 'retired requests not released');
      const current = guest();
      current.evaluate(lifecycleScripts.start + ';void 0;'); await drive(current);
      const reply = host.stalled.find(r => r.generation === current.generation);
      const subscription = [...host.subscriptions].find(([,g]) => g === current.generation)[0];
      check(reply.id === oldReply.id, 'request IDs must collide for regression test');
      check(!current.deliver({id: oldReply.id, value: 'stale'}, old.generation), 'stale response accepted');
      check(!current.deliver({event: subscription, value: 99}, old.generation), 'stale event accepted');
      check(!current.deliver({event: oldSubscription, value: 99}, old.generation), 'old subscription accepted');
      check(!old.deliver({id: oldReply.id, value: 'stale'}), 'disposed runtime accepted response');
      await drive(current);
      check(current.evaluate('JSON.stringify([reply,events.length])') === '[null,0]', 'stale delivery mutated new state');
      current.deliver({id: reply.id, value: 'new'});
      current.deliver({event: subscription, value: 3});
      await drive(current); current.evaluate(lifecycleScripts.verify);
      const survivorSub = [...host.subscriptions].find(([,g]) => g === survivor.generation)[0];
      survivor.deliver({event: survivorSub, value: 8});
      check(survivor.evaluate('JSON.stringify(events)') === '[8]', 'unrelated instance affected');
      current.dispose(); survivor.dispose();
      check(host.subscriptions.size === 0 && host.stalled.length === 0, 'host resource leak');
      return {forced_reload_cleanup:true, stale_response_rejected:true, stale_event_rejected:true,
        colliding_request_ids:true, other_instance_survives:true};
    }

    const lifecycle = await checkLifecycle();
    let checks;
    for (let i = 0; i < 20; i++) {
      const g = guest();
      try {
        g.evaluate(suite + ';void 0;'); checks = (await drive(g)).checks;
        g.evaluate('vm.state.value=9; vm.action=()=>10;');
        check(g.evaluate('JSON.stringify([vm.state.value,vm.action()])') === '[9,10]', 'live patch failed');
        check(host.subscriptions.size === 0, 'subscription leak');
      } finally { g.dispose(); }
    }
    const clean = guest();
    try { check(clean.evaluate('JSON.stringify([vm.state.value,vm.action()])') === '[1,2]' && host.value === 7, 'reload failed'); }
    finally { clean.dispose(); }
    const runaway = guest(); let interrupted = false;
    const deadline = performance.now() + 50;
    runaway.rt.setInterruptHandler(() => performance.now() > deadline);
    const t = performance.now();
    try { runaway.evaluate('while(true){}'); } catch { interrupted = true; } finally { runaway.dispose(); }
    check(interrupted, 'runaway not interrupted');
    const interruption_ms = performance.now() - t;
    const memoryComparison = {
      bellard_0_31_0: probeMemory(await oldModule(oldVariant(OLD, {wasmLocation:'/web/dist/quickjs.wasm'}))),
      bellard_0_32_0: probeMemory(await newModule(newVariant(NEW, {wasmLocation:'/web/dist/new.wasm'}))),
      quickjs_ng_0_32_0: probeMemory(Q),
    };
    const capped_memory = await probeCappedMemory('/web/dist/ng.wasm');
    const heapLimit = memoryComparison.quickjs_ng_0_32_0.every(p => p.rejected && /memory/i.test(p.error?.message || ''));
    const after = guest(); try { check(after.evaluate('1+1') === 2, 'unrelated context failed'); } finally { after.dispose(); }
    self.postMessage({ host:'browser-wasm', variant:'quickjs-ng 0.32.0', cycles:20, checks, lifecycle,
      fresh_context:true, engine_effect_survives:true, interruption_ms, heap_limit:heapLimit,
      memory_comparison:memoryComparison, capped_memory, qualified:heapLimit, elapsed_ms:performance.now()-start });
  } catch (error) { self.postMessage({error:String(error.stack || error)}); }
};
