import {newQuickJSWASMModule,newVariant} from 'quickjs-new';
import NG from '@jitl/quickjs-ng-wasmfile-release-sync';
let runtime,vm,deadline,active=0,peak=0,requests=0,finished=false;
const memory=new WebAssembly.Memory({initial:256,maximum:256});
const session='browser-'+crypto.randomUUID();
function evaluate(source){
  const result=vm.evalCode(source);
  if(result.error){const error=vm.dump(result.error);result.error.dispose();throw Error(JSON.stringify(error));}
  const value=vm.dump(result.value);result.value.dispose();return value;
}
function pump(){
  deadline=performance.now()+500;
  const jobs=runtime.executePendingJobs(10000);
  if(jobs.error){jobs.error.dispose();throw Error('pending job error');}
  if(runtime.hasPendingJob())throw Error('job budget');
  const report=evaluate('report');
  if(report.done && active===0 && !finished){
    finished=true;self.postMessage({...report,host:'browser',peak_http_requests:peak,http_requests_remaining:active,memory_bytes:memory.buffer.byteLength});
    vm.dispose();runtime.dispose();
  }
}
async function request(raw){
  if(raw.length>32768 || active>=32 || ++requests>2000)throw Error('wire/request budget');
  const message=JSON.parse(raw);
  if(Object.keys(message).sort().join(',')!=='args,channel,epoch,id,op')throw Error('request envelope');
  active++;peak=Math.max(peak,active);
  let reply;
  try {
    const response=await fetch('/rpc',{method:'POST',headers:{'content-type':'application/json'},
      body:JSON.stringify({...message,session}),signal:AbortSignal.timeout(8000)});
    if(!response.ok)throw Error('HTTP failure');
    const text=await response.text();if(text.length>65536)throw Error('response budget');
    reply=JSON.parse(text);
    if(['channel','epoch','id'].some(key=>reply[key]!==message[key]))throw Error('response route mismatch');
  }catch(error){reply={channel:message.channel,epoch:message.epoch,id:message.id,transportError:true};}
  active--;
  deadline=performance.now()+500;
  evaluate(`__receive(${JSON.stringify(JSON.stringify(reply))})`);pump();
}
self.onmessage=async({data})=>{
  try {
    const Q=await newQuickJSWASMModule(newVariant(NG,{wasmMemory:memory,wasmLocation:'/ng.wasm'}));
    runtime=Q.newRuntime();runtime.setMemoryLimit(16*1024*1024);runtime.setMaxStackSize(512*1024);
    runtime.setInterruptHandler(()=>performance.now()>deadline);vm=runtime.newContext();
    const send=vm.newFunction('__send',raw=>{
      if(vm.typeof(raw)!=='string')throw Error('wire type');
      // Browser networking belongs to the host Worker, outside authored QuickJS.
      request(vm.getString(raw)).catch(error=>self.postMessage({error:String(error)}));
    });
    vm.setProp(vm.global,'__send',send);send.dispose();deadline=performance.now()+500;
    evaluate(data.client+';void 0;');evaluate(data.suite+';void 0;');pump();
  }catch(error){self.postMessage({error:String(error.stack||error)});}
};
