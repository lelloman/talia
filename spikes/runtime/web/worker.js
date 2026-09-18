import { newQuickJSWASMModule, newVariant, RELEASE_SYNC } from 'quickjs-emscripten';
self.onmessage = async () => {
 try {
  const [bridge,suite]=await Promise.all(['bridge.js','suite.js'].map(n=>fetch('/shared/'+n).then(r=>r.text())));
  const Q=await newQuickJSWASMModule(newVariant(RELEASE_SYNC,{wasmLocation:'/web/dist/quickjs.wasm'}));
  const host={value:0,subscriptions:new Set(),next:0,stalled:[]};
  const start=performance.now();
  function guest() {
   const rt=Q.newRuntime();rt.setMemoryLimit(16*1024*1024);rt.setMaxStackSize(512*1024);
   const until=performance.now()+3000;rt.setInterruptHandler(()=>performance.now()>until);
   const vm=rt.newContext(),out=[];
   const send=vm.newFunction('__send',raw=>{out.push(JSON.parse(vm.getString(raw)));});
   vm.setProp(vm.global,'__send',send);send.dispose();
   function evaluate(code) {
    const r=vm.evalCode(code);
    if(r.error){const e=vm.dump(r.error);r.error.dispose();throw Error(JSON.stringify(e));}
    const v=vm.dump(r.value);r.value.dispose();return v;
   }
   const deliver=m=>evaluate(`__receive(${JSON.stringify(JSON.stringify(m))})`);
   evaluate(bridge);
   return {rt,vm,out,evaluate,deliver,dispose(){vm.dispose();rt.dispose();}};
  }
  async function drive(g) {
   const until=performance.now()+4000;
   while(performance.now()<until) {
    const jobs=g.rt.executePendingJobs(10000);
    if(jobs.error){const e=g.vm.dump(jobs.error);jobs.error.dispose();throw Error(JSON.stringify(e));}
    const req=g.out.shift();
    if(!req){const r=JSON.parse(g.evaluate('JSON.stringify(report)'));if(r.done){if(r.error)throw Error(r.error);return r;}await new Promise(r=>setTimeout(r,1));continue;}
    const {id,op,value}=req;let result=null;
    switch(op){
     case 'echo':result=value;break;
     case 'delay':await new Promise(r=>setTimeout(r,5));break;
     case 'read':result=host.value;break;
     case 'write':result=host.value=value;break;
     case 'fail':g.deliver({id,error:'host failure'});continue;
     case 'subscribe':result='s'+(++host.next);host.subscriptions.add(result);break;
     case 'unsubscribe':host.subscriptions.delete(value);break;
     case 'publish':for(const s of host.subscriptions)g.deliver({event:s,value});break;
     case 'stall':host.stalled.push(id);continue;
     case 'late':for(const old of host.stalled.splice(0))g.deliver({id:old,value:999});break;
     default:throw Error('unknown capability');
    }
    g.deliver({id,value:result});if(op==='late')g.deliver({id,value:result});
   }
   throw Error('host deadline exceeded');
  }
  let checks;
  for(let i=0;i<20;i++){
   const g=guest();
   try{
    g.evaluate(suite+';void 0;');checks=(await drive(g)).checks;
    g.evaluate('vm.state.value=9; vm.action=()=>10;');
    if(g.evaluate('JSON.stringify([vm.state.value,vm.action()])')!=='[9,10]')throw Error('live patch failed');
    if(host.subscriptions.size)throw Error('subscription leak');
   }finally{g.dispose();host.subscriptions.clear();host.stalled=[];}
  }
  const clean=guest();try{if(clean.evaluate('JSON.stringify([vm.state.value,vm.action()])')!=='[1,2]'||host.value!==7)throw Error('reload failed');}finally{clean.dispose();}
  const runaway=guest();let interrupted=false;const deadline=performance.now()+50;
  runaway.rt.setInterruptHandler(()=>performance.now()>deadline);const t=performance.now();
  try{runaway.evaluate('while(true){}');}catch{interrupted=true;}finally{runaway.dispose();}
  if(!interrupted)throw Error('runaway not interrupted');const interruption_ms=performance.now()-t;
  const alloc=guest();let heapLimit=false;
  try{alloc.evaluate('globalThis.buffers=[];for(let i=0;i<64;i++)buffers.push(new ArrayBuffer(1024*1024));');}catch{heapLimit=true;}finally{alloc.dispose();}
  const objects=guest(); let objectLimit=false;
  try{objects.evaluate('globalThis.items=[];for(let i=0;i<1000000;i++)items.push({i,text:String(i)});');}catch{objectLimit=true;}finally{objects.dispose();}
  const after=guest();try{if(after.evaluate('1+1')!==2)throw Error('unrelated context failed');}finally{after.dispose();}
  self.postMessage({host:'browser-wasm',cycles:20,checks,fresh_context:true,engine_effect_survives:true,interruption_ms,heap_limit:heapLimit,object_heap_limit:objectLimit,qualified:heapLimit&&objectLimit,elapsed_ms:performance.now()-start});
 }catch(e){self.postMessage({error:String(e.stack||e)});}
};
