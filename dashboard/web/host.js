export class Guest {
 constructor(onRequests,onFailure){
  this.worker=new Worker('/dashboard/web/dist/worker.js',{type:'module'});this.pending=new Map();this.next=0;this.alive=true;this.onRequests=onRequests;this.onFailure=onFailure;
  this.worker.onmessage=({data:r})=>{
   if(!this.alive)return;if(r.error){this.close(r.error);return;}
   const p=this.pending.get(r.id);if(!p)return;clearTimeout(p.timer);this.pending.delete(r.id);p.resolve(r.value);if(r.out?.length)this.onRequests(r.out);
  };
  this.worker.onerror=e=>{e.preventDefault();this.close('Worker failure');};
 }
 eval(source,init=false){return new Promise((resolve,reject)=>{
  if(!this.alive){reject(Error('guest stopped'));return;}
  if(this.pending.size>=64){this.close('host command budget');reject(Error('host command budget'));return;}
  const id=++this.next,timer=setTimeout(()=>this.close('Guest watchdog'),3000);this.pending.set(id,{resolve,reject,timer});this.worker.postMessage({id,source,init});
 });}
 close(reason=null){if(!this.alive)return;this.alive=false;this.worker.terminate();for(const p of this.pending.values()){clearTimeout(p.timer);p.reject(Error(reason||'closed'));}this.pending.clear();if(reason)this.onFailure(reason);}
}
export class EngineBridge {
 constructor(deliver,changed){this.deliver=deliver;this.changed=changed;this.epoch=1;this.id=0;this.last=0;this.subscriptions=new Map();this.actions=new Map();this.nextActionId='a-'+crypto.randomUUID();this.paused=false;this.closed=false;this.controllers=new Set();this.timer=setInterval(()=>this.poll(),250);}
 async rpc(op,args){
  if(this.closed||this.paused)throw Error('cancelled');
  if(this.controllers.size>=64)throw Error('I/O queue limit');
  const epoch=this.epoch,controller=new AbortController();this.controllers.add(controller);const timer=setTimeout(()=>controller.abort(),5000);
  try{
   const r=await fetch('/rpc',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({session:'p1-dashboard',channel:'dashboard',epoch,id:++this.id,op,args}),signal:controller.signal});
   if(!r.ok)throw Error('HTTP '+r.status);const msg=await r.json();if(epoch!==this.epoch||this.closed||this.paused)throw Error('cancelled');if(msg.error)throw Error(msg.error);return msg.value;
  }finally{clearTimeout(timer);this.controllers.delete(controller);}
 }
 async requests(requests){for(const r of requests){
  if(this.closed||this.paused)return;
  if(!Number.isSafeInteger(r.id)||r.id<=this.last){this.changed('bridge request replay');return;}this.last=r.id;
  const epoch=this.epoch;
  const run=async()=>{
   let value;
   switch(r.op){
    case 'read':if(r.value!=='value')throw Error('unknown resource');value=await this.rpc('read',{tag:'ui-read'});break;
    case 'write':{
     if(!Number.isInteger(r.value)||Math.abs(r.value)>1000000)throw Error('write value');
     if(this.actions.size>=128)throw Error('action tracking limit');
     const actionId=this.nextActionId;this.nextActionId='a-'+crypto.randomUUID();this.actions.set(actionId,{status:'unknown'});value=await this.rpc('action',{actionId,value:r.value});this.actions.set(actionId,value);break;
    }
    case 'subscribe':if(r.value!=='value')throw Error('unknown resource');if(this.subscriptions.size>=16)throw Error('subscription limit');value='s'+r.id;this.subscriptions.set(value,{revision:-1});break;
    case 'unsubscribe':if(!this.subscriptions.delete(r.value))throw Error('subscription ownership');value=null;break;
    default:this.changed('operation not granted');return;
   }
   if(epoch===this.epoch&&!this.paused&&!this.closed)await this.deliver({id:r.id,value});
  };
  run().catch(e=>{if(epoch===this.epoch&&!this.paused&&!this.closed)this.deliver({id:r.id,error:String(e)}).catch(()=>{});});
 }}
 async poll(){
  if(this.polling||this.paused||this.closed||!this.subscriptions.size)return;this.polling=true;
  try{const value=await this.rpc('read',{tag:'ui-subscription'});for(const [event,sub]of this.subscriptions)if(value.revision>sub.revision){sub.revision=value.revision;await this.deliver({event,value});}}
  catch(e){this.externalError=String(e);}finally{this.polling=false;}
 }
 pause(){this.paused=true;this.epoch++;for(const c of this.controllers)c.abort();}
 async resume(){if(this.closed)return;this.paused=false;this.epoch++;for(const s of this.subscriptions.values())s.revision=-1;
  for(const id of this.actions.keys()){try{this.actions.set(id,await this.rpc('status',{actionId:id}));}catch(e){this.externalError=String(e);}}await this.poll();
 }
 close(){this.closed=true;this.pause();clearInterval(this.timer);this.subscriptions.clear();}
}
