import {DashboardHost} from '../runtime/web/dashboard-host.js';
const status=document.querySelector('#status'), host=new DashboardHost();
const [bridge,saved]=await Promise.all(['/shared/bridge.js','/lifecycle/dashboard.js'].map(p=>fetch(p).then(r=>r.text())));
const survivor=await host.create(bridge,[]);let survivor_ticks=0;
let guest=await host.create(bridge,['read','write']);await guest.eval(saved+';void 0;');
let epoch=0,next=0,pending=0,ignored=0,snapshot=null,local={dirty:false,ticks:0},outcomes={},polling=false;
let failure=null,external_error=null;const signals=[];
host.onFailure=(g,reason)=>{if(g!==guest || failure)return;failure=reason;epoch++;if(document.querySelector('#signals').checked)signals.push({type:'dashboard-failure',diagnostic:reason});};
const session='lifecycle-browser';
let actionSequence=0;const actionPrefix=crypto.randomUUID();window.nextActionId=()=>`a-${actionPrefix}-${actionSequence+1}`;
async function rpc(op,args){
 const request={session,channel:'dashboard',epoch,id:++next,op,args};pending++;
 try{const r=await fetch('/rpc',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(request)});if(!r.ok)throw Error('HTTP failure');return await r.json();}
 catch(e){external_error=String(e);return {error:external_error};}finally{pending--;}
}
function apply(data){const s=data?.revision!==undefined?data:data?.result;if(s && (!snapshot || s.revision>=snapshot.revision))snapshot=s;}
host.remote=async(g,request)=>{
 const e=epoch,action=request.op==='write'?`a-${actionPrefix}-${++actionSequence}`:null;
 if(action)outcomes[action]={status:'unknown'};
 const reply=await rpc(action?'action':'read',action?{actionId:action,value:request.value}:{tag:'guest'});
 if(e!==epoch || guest.paused){ignored++;return;}
 if(action)outcomes[action]=reply.value;apply(reply.value);
 host.deliver(g,reply.error?{id:request.id,error:reply.error}:{id:request.id,value:reply.value});
};
async function visibility(){
 if(failure)return;
 epoch++;
 if(document.hidden){host.pause(guest);return;}
 await host.resume(guest);
 const e=epoch,reply=await rpc('read',{tag:'resume'});
 if(e!==epoch || guest.paused){ignored++;return;}apply(reply.value);
 for(const id of Object.keys(outcomes)){const result=await rpc('status',{actionId:id});if(e!==epoch || guest.paused){ignored++;return;}outcomes[id]=result.value;}
}
document.addEventListener('visibilitychange',()=>visibility().catch(e=>status.textContent=String(e)));
await visibility();
document.querySelector('#dirty').onclick=()=>guest.eval('vm.state.dirty=true;vm.action=()=>10;void 0;').catch(()=>{});
document.querySelector('#action').onclick=()=>guest.eval('writeValue(42);void 0;').catch(()=>{});
document.querySelector('#restart').onclick=async()=>{if(!failure)return;host.retire(guest,'manual restart');guest=await host.create(bridge,['read','write']);await guest.eval(saved+';void 0;');failure=null;local={dirty:false,ticks:0};await visibility();};
for(const [id,source] of Object.entries({crash:"throw Error('fixture internal error')",runaway:'while(true){}',memory:'globalThis.buffers=[];for(let i=0;i<1000;i++)buffers.push(new ArrayBuffer(1024*1024));'}))document.querySelector('#'+id).onclick=()=>guest.eval(source+';void 0;').catch(()=>{});
document.querySelector('#external').onclick=async()=>{const reply=await rpc('unsupported-probe',{});external_error=reply.error??null;};
setInterval(async()=>{
 if(polling)return;polling=true;
 try{
  if(!guest.paused && guest.active){const e=epoch;const reply=await rpc('read',{tag:'subscription'});if(e===epoch && !guest.paused){apply(reply.value);local=JSON.parse(await guest.eval('vm.state.ticks++;JSON.stringify(vm.state)'));}else ignored++;}
  if(!document.hidden){await survivor.eval('1+1');survivor_ticks++;}
  window.lifecycleReport={survivor_ticks,failure,external_error,signals,pending_waits:guest.active?guest.pending.size:0,active:guest.active&&!guest.paused,epoch,local,snapshot,outcomes,pending,ignored,subscriptions:guest.active&&!guest.paused?1:0};status.textContent=failure?'Dashboard stopped — internal error\n'+failure:JSON.stringify(window.lifecycleReport);status.style.color=failure?'red':'';status.setAttribute('role',failure?'alert':'status');document.querySelector('#restart').hidden=!failure;
 }catch(e){if(!guest.paused)status.textContent=String(e);}finally{polling=false;}
},100);
