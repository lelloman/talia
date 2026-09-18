import {newQuickJSWASMModule, newVariant} from 'quickjs-new';
import {LIMITS, boundedText, validateRequest} from './bridge-policy.js';
import NG from '@jitl/quickjs-ng-wasmfile-release-sync';

// Test host only: one module and one guest per disposable Worker.
let memory, runtime, vm, deadline;
let outstanding = 0, poisoned = false;
function violation(error) {
  if (!poisoned) self.postMessage({fatal:'bridge policy: ' + String(error).slice(0, 256)});
  poisoned = true;
}
function evaluate(source) {
  const result = vm.evalCode(source);
  if (result.error) {
    let diagnostic = null;
    try { diagnostic = vm.dump(result.error); } finally { result.error.dispose(); }
    throw Error(JSON.stringify(diagnostic));
  }
  try { return vm.dump(result.value); } finally { result.value.dispose(); }
}
self.onmessage = async ({data: {id, op, value}}) => {
  if (poisoned) return;
  if (op === 'ack') { outstanding = Math.max(0, outstanding - 1); return; }
  try {
    deadline = performance.now() + 500;
    let result;
    if (op === 'init') {
      memory = new WebAssembly.Memory({initial:256, maximum:256});
      const Q = await newQuickJSWASMModule(newVariant(NG, {
        wasmMemory:memory, wasmLocation:'/web/dist/ng.wasm',
      }));
      runtime = Q.newRuntime();
      runtime.setMemoryLimit(16 * 1024 * 1024);
      runtime.setMaxStackSize(512 * 1024);
      runtime.setInterruptHandler(() => performance.now() > deadline);
      vm = runtime.newContext();
      const send = vm.newFunction('__send', raw => {
        try {
          if (poisoned) throw Error('retired bridge');
          if (outstanding >= LIMITS.inFlight) throw Error('in-flight request limit');
          if (vm.typeof(raw) !== 'string') throw Error('wire size/type');
          const text = vm.getString(raw);
          validateRequest(text);
          outstanding++;
          self.postMessage({request:text});
        } catch (error) { violation(error); throw Error('bridge policy violation'); }
      });
      vm.setProp(vm.global, '__send', send); send.dispose();
      evaluate(value);
    } else if (op === 'eval') result = evaluate(value);
    else if (op === 'deliver') evaluate(`__receive(${JSON.stringify(JSON.stringify(value))})`);
    else if (op === 'interrupt') {
      deadline = performance.now() + 50;
      result = evaluate('while(true){}');
    } else if (op === 'hang') {
      // Trusted harness fault injection: deliberately bypass QuickJS interruption.
      self.postMessage({hanging:true});
      while (true) { /* Parent must terminate us without guest cleanup. */ }
    } else throw Error('unknown harness command');
    const jobs = runtime.executePendingJobs(10000);
    if (jobs.error) { jobs.error.dispose(); throw Error('pending job failed'); }
    if (runtime.hasPendingJob()) throw Error('pending job budget exceeded');
    const response = {id, value:result, memory_bytes:memory.buffer.byteLength};
    boundedText(JSON.stringify(response), LIMITS.commandBytes);
    if (!poisoned) self.postMessage(response);
  } catch (error) {
    // A null guest exception under OOM is still fatal. Never depend on guest cleanup.
    self.postMessage({fatal:String(error).slice(0, 512), memory_bytes:memory?.buffer.byteLength});
  }
};
