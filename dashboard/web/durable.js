import '../../engine/shared/value.js';import '../../engine/shared/client.js';
const status=document.querySelector('#connection');
const client=new TaliaConnection({client:'web-'+crypto.randomUUID(),send:async body=>{
 const r=await fetch('/engine',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body),signal:AbortSignal.timeout(5000)});if(!r.ok)throw Error('HTTP '+r.status);return r.json();
},status:s=>{status.textContent=s;status.hidden=!s;}});
setInterval(()=>client.tick(),500);client.tick();
export class DurableBridge {
 constructor(deliver,changed,grants={reads:['value'],writes:['value'],runs:[]}){Object.assign(this,{deliver,changed,grants,subscriptions:new Map(),actions:client.actions,nextActionId:'a-'+crypto.randomUUID(),paused:false,closed:false,last:0});client.onSnapshot=()=>this.poll();client.setResources([]).catch(()=>{});}
 allowed(kind,id){if(!(this.grants[kind]||[]).includes(id))throw Error('resource grant');}
 async sync(){await client.setResources([...this.subscriptions.values()].map(s=>s.resource));await client.setActive(!this.paused&&!this.closed&&this.subscriptions.size>0);}
 async requests(requests){for(const r of requests){if(this.closed||this.paused)return;if(!Number.isSafeInteger(r.id)||r.id<=this.last){this.changed('bridge replay');return;}this.last=r.id;
  const run=async()=>{let value;switch(r.op){
   case 'subscribe':this.allowed('reads',r.value);if(this.subscriptions.size>=16)throw Error('subscription budget');value='s'+r.id;this.subscriptions.set(value,{resource:r.value,key:null,ready:false});await this.sync();break;
   case 'unsubscribe':if(!this.subscriptions.delete(r.value))throw Error('subscription ownership');await this.sync();value=null;break;
   case 'read':this.allowed('reads',r.value);value=await client.read(r.value);break;
   case 'write':{const resource=r.value?.id??'value';this.allowed('writes',resource);const input=r.value?.wire?TaliaValue.decode(r.value.wire):r.value;const id=this.nextActionId;this.nextActionId='a-'+crypto.randomUUID();value=await client.write(input,id,resource);break;}
   case 'run':{this.allowed('runs',r.value);const id=this.nextActionId;this.nextActionId='a-'+crypto.randomUUID();value=await client.run(r.value,id);break;}
   default:this.changed('operation not granted');return;
  }if(!this.closed&&!this.paused)await this.deliver({id:r.id,valueWire:TaliaValue.encode(value)});if(r.op==='subscribe'&&this.subscriptions.has(value))this.subscriptions.get(value).ready=true;};run().catch(e=>{if(!this.closed&&!this.paused)this.deliver({id:r.id,error:String(e)}).catch(()=>{});});}}
 async poll(){if(this.closed||this.paused||!client.ready)return;for(const [event,sub]of this.subscriptions){if(!sub.ready)continue;let value;try{value=client.sample(sub.resource);}catch{continue;}const key=client.incarnation+':'+TaliaValue.stringify(value);if(key!==sub.key){sub.key=key;await this.deliver({event,valueWire:TaliaValue.encode(value)});}}}
 pause(){this.paused=true;client.setActive(false).catch(()=>{});}
 async resume(){this.paused=false;this.subscriptions.forEach(s=>s.key=null);await this.sync();await client.tick();await this.poll();}
 close(){this.closed=true;this.subscriptions.clear();client.setActive(false).catch(()=>{});}
}
export {client};
