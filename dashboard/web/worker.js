import {newQuickJSWASMModule,newVariant} from 'quickjs-new';
import NG from '@jitl/quickjs-ng-wasmfile-release-sync';
let runtime,ctx,until,poisoned=false;const out=[];
function evaluate(source){
 if(typeof source!=='string'||source.length>262144)throw Error('source limit');
 const r=ctx.evalCode(source);if(r.error){let e;try{e=ctx.dump(r.error)}finally{r.error.dispose()}throw Error(JSON.stringify(e));}
 try{return ctx.dump(r.value)}finally{r.value.dispose()}
}
async function handle({data:{id,source,init}}){
 if(poisoned)return;
 try{
  until=performance.now()+500;
  if(init){
   const memory=new WebAssembly.Memory({initial:256,maximum:256});
   const q=await newQuickJSWASMModule(newVariant(NG,{wasmMemory:memory,wasmLocation:'/dashboard/web/dist/ng.wasm'}));
   runtime=q.newRuntime();runtime.setMemoryLimit(16*1024*1024);runtime.setMaxStackSize(512*1024);runtime.setInterruptHandler(()=>performance.now()>until);ctx=runtime.newContext();
   const send=ctx.newFunction('__send',raw=>{
    try{
     if(ctx.typeof(raw)!=='string')throw Error('wire type');const text=ctx.getString(raw);if(text.length>32768||out.length>=64)throw Error('bridge budget');const r=JSON.parse(text);
     if(Object.keys(r).length!==3||!Number.isSafeInteger(r.id)||r.id<1||!['read','write','subscribe','unsubscribe'].includes(r.op))throw Error('capability');
     out.push(r);
    }catch(e){poisoned=true;throw e;}
   });ctx.setProp(ctx.global,'__send',send);send.dispose();until=performance.now()+500;
  }
  const value=evaluate(source);const jobs=runtime.executePendingJobs(10000);if(jobs.error){jobs.error.dispose();throw Error('pending job failed');}
  if(poisoned||runtime.hasPendingJob())throw Error('guest budget/bridge violation');
  const result={id,value,out:out.splice(0)};if(JSON.stringify(result).length>262144)throw Error('response size limit');self.postMessage(result);
 }catch(e){poisoned=true;self.postMessage({id,error:String(e)});}
};

let commands=Promise.resolve();
self.onmessage=event=>{commands=commands.then(()=>handle(event));};
