import {Guest} from './host.js';
const wait=ms=>new Promise(r=>setTimeout(r,ms));
export class LiveControl {
 constructor(registry,host){Object.assign(this,{registry,host,busy:false});setInterval(()=>this.poll(),200);}
 async poll(){if(this.busy||!this.registry.connected)return;this.busy=true;try{const address=this.registry.address();const r=await this.registry.send({op:'livePoll',...address});for(const c of r.value.commands)await this.execute(c,address);}catch{}finally{this.busy=false;}}
 async execute(c,address){
  const h=this.host,live=address.live,until=performance.now()+Math.min(5000,c.remainingMs),args=c.arguments;
  const send=(op,extra={})=>this.registry.send({op,...address,commandId:c.id,...extra}).then(r=>r.value);
  const local=()=>{if(performance.now()>=until||document.hidden||this.registry.slot.live!==live||(c.operation==='live_execute'&&h.report().lifecycle==='failed'))throw Error('cancelled');};
  const check=async()=>{local();await send('liveCheck');local();};
  let invocation;
  try{
   local();const report=h.report();if(args.expectedEditRevision!==undefined&&args.expectedEditRevision!==report.editRevision)throw Error('conflict');
   const begun=await send('liveBegin',{report:{liveInstanceId:live,...report},grants:h.grants()});local();
   if(c.operation==='live_inspect'){await send('liveFinish',{value:{snapshot:await h.inspect(),...h.metadata()}});return;}
   if(c.operation==='live_reload'){
    await h.reload(begun.delivery,async()=>{await check();const r=h.report();if(r.editRevision!==args.expectedEditRevision)throw Error('conflict');if(r.dirty&&!args.discardDirty)throw Error('dirty_ack_required');});
    while(this.registry.busy&&performance.now()<until)await wait(10);await this.registry.tick(true);
    await send('liveFinish',{value:{...h.metadata(),error:h.report().lifecycle==='failed'?'validation_failed':undefined}});return;
   }
   h.dirty();await h.markDirty();const snapshot=await h.inspect();await check();
   const initial={revision:snapshot.revision,value:TaliaValue.decode(snapshot.state)};
   const codec=await (await fetch('/engine/shared/value.js')).text(),library=await (await fetch('/dashboard/shared/live.js')).text();await check();
   let broken=null;
   invocation=new Guest(requests=>{for(const r of requests){(async()=>{
    await check();let value;
    if(r.op==='commit'){value=await h.commit(r.value.revision,r.value.value);}
    else{const id=r.op==='write'?r.value.id:r.value;if(!['read','write','run'].includes(r.op))throw Error('forbidden');value=await send('liveEffect',{effect:{op:r.op,id,value:r.op==='write'?r.value.wire:undefined,sequence:r.id}});if(r.op!=='run'&&value.value)value={...value,value:TaliaValue.decode(value.value)};}
    await check();await invocation.eval(`TaliaLive.receive(${JSON.stringify(JSON.stringify({id:r.id,value:TaliaValue.encode(value)}))});'ok';`);
   })().catch(async e=>{try{await check();await invocation.eval(`TaliaLive.receive(${JSON.stringify(JSON.stringify({id:r.id,error:String(e)}))});'ok';`);}catch{broken='cancelled';}});}},e=>{broken='validation_failed';});
   await invocation.eval(codec+'\nconst liveInitial=TaliaValue.decode('+JSON.stringify(TaliaValue.encode(initial))+');\n'+library+'\nTaliaLive.start('+JSON.stringify(args.source)+');\'ok\';',true);
   for(;;){await check();if(broken)throw Error(broken);const result=JSON.parse(await invocation.eval('TaliaLive.snapshot()'));if(result){if(result.error)h.fail('Live execution failed');await send('liveFinish',{value:{...h.metadata(),error:result.error}});break;}await wait(20);}
  }catch(e){const code=['conflict','dirty_ack_required','cancelled'].find(x=>String(e).includes(x))||'validation_failed';if(code==='validation_failed'&&(c.operation==='live_execute'||c.operation==='live_reload'&&this.registry.slot.live!==live))h.fail('Live execution failed');try{await send('liveFinish',{value:{...h.metadata(),error:code}});}catch{}}
  finally{invocation?.close();}
 }
}
