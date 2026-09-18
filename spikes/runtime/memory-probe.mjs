// Bounded reproduction: all allocations are retained; never run an unbounded OOM loop.
export const memoryCases = {
    single_buffer: 'globalThis.keep = [new ArrayBuffer(32*1024*1024)];',
    repeated_buffers: 'globalThis.keep=[];for(let i=0;i<64;i++)keep.push(new ArrayBuffer(1024*1024));',
    repeated_strings: 'globalThis.keep=[];for(let i=0;i<8;i++)keep.push(String(i)+"x".repeat(4*1024*1024));',
    objects: 'globalThis.keep=[];for(let i=0;i<400000;i++)keep.push({i,text:String(i)});',
};
export function probeMemory(Q) {
  const MiB = 1024 * 1024;
  return Object.entries(memoryCases).map(([name, source]) => {
    const rt = Q.newRuntime();
    rt.setMemoryLimit(16 * MiB);
    const vm = rt.newContext();
    const deadline = Date.now() + 5000;
    rt.setInterruptHandler(() => Date.now() > deadline);
    let rejected = false, error = null, retained = null, usage = null;
    try {
      const result = vm.evalCode(source + '\nvoid 0;');
      if (result.error) {
        rejected = true;
        error = vm.dump(result.error);
        result.error.dispose();
      } else result.value.dispose();
      const size = vm.evalCode('keep.length');
      if (size.error) size.error.dispose();
      else { retained = vm.dump(size.value); size.value.dispose(); }
      const memory = rt.computeMemoryUsage();
      usage = vm.dump(memory); memory.dispose();
    } finally { vm.dispose(); rt.dispose(); }
    return {name, limit_bytes:16*MiB, rejected, error, retained,
      memory_used_bytes:usage?.memory_used_size, malloc_size:usage?.malloc_size};
  });
}
