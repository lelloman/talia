import {newQuickJSWASMModule, newVariant} from 'quickjs-new';
import NG from '@jitl/quickjs-ng-wasmfile-release-sync';
import {memoryCases} from './memory-probe.mjs';

// Each budget belongs to an entire WASM module, never to a shared guest context.
export async function probeCappedMemory(wasmLocation) {
  const probes = [];
  for (const [name, source] of Object.entries(memoryCases)) {
    const memory = new WebAssembly.Memory({initial:256, maximum:256});
    const Q = await newQuickJSWASMModule(newVariant(NG, {
      wasmMemory:memory, ...(wasmLocation ? {wasmLocation} : {}),
    }));
    const rt = Q.newRuntime(); rt.setMemoryLimit(16 * 1024 * 1024);
    const deadline = Date.now() + 5000;
    rt.setInterruptHandler(() => Date.now() > deadline);
    const vm = rt.newContext();
    let rejected = false, error = null, disposed = false, failure = null;
    try {
      const result = vm.evalCode(source + ';void 0;');
      if (result.error) {
        rejected = true; error = vm.dump(result.error); result.error.dispose();
      } else result.value.dispose();
    } catch (e) { failure = String(e); }
    try { vm.dispose(); rt.dispose(); disposed = true; }
    catch (e) { failure = String(e); }
    let growth_rejected = false;
    try { memory.grow(1); } catch (e) { growth_rejected = e instanceof RangeError; }
    probes.push({name, rejected, error, disposed, failure, growth_rejected,
      memory_bytes:memory.buffer.byteLength});
    // A failed module must be discarded; recovery uses a separate module below.
  }
  const memory = new WebAssembly.Memory({initial:256, maximum:256});
  const Q = await newQuickJSWASMModule(newVariant(NG, {
    wasmMemory:memory, ...(wasmLocation ? {wasmLocation} : {}),
  }));
  const vm = Q.newContext(), result = vm.evalCode('1+1');
  const fresh_module_works = !result.error && vm.dump(result.value) === 2;
  (result.error || result.value).dispose(); vm.dispose();
  return {probes, fresh_module_works};
}
