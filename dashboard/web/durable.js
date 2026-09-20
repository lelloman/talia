import '../../engine/shared/value.js';import '../../engine/shared/client.js';
const status=document.querySelector('#connection');
const client=new TaliaConnection({client:'web-'+crypto.randomUUID(),send:async body=>{
 const r=await fetch('/engine',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body),signal:AbortSignal.timeout(5000)});if(!r.ok)throw Error('HTTP '+r.status);return r.json();
},status:s=>{status.textContent=s;status.hidden=!s;}});
setInterval(()=>client.tick(),500);client.tick();
export class DurableBridge {
 constructor(deliver,changed){Object.assign(this,{deliver,changed,subscriptions:new Map(),actions:client.actions,nextActionId:'a-'+crypto.randomUUID(),paused:false,closed:false,last:0});client.onSnapshot=()=>this.poll();}
 async requests(requests){for(const r of requests){if(this.closed||this.paused)return;if(!Number.isSafeInteger(r.id)||r.id<=this.last){this.changed('bridge replay');return;}this.last=r.id;const run=async()=>{let value;switch(r.op){
 case 'subscribe':if(r.value!=='value'||this.subscriptions.size>=16)throw Error('resource grant/budget');value='s'+r.id;await client.setActive(true);this.subscriptions.set(value,-1);break;
 case 'unsubscribe':if(!this.subscriptions.delete(r.value))throw Error('subscription ownership');if(!this.subscriptions.size)await client.setActive(false);value=null;break;
 case 'read':if(r.value!=='value')throw Error('resource grant');value=await client.read();break;
 case 'write':{const input=r.value?.wire?TaliaValue.decode(r.value.wire):r.value;const id=this.nextActionId;this.nextActionId='a-'+crypto.randomUUID();value=await client.write(input,id);break;}
 default:this.changed('operation not granted');return;
 }if(!this.closed&&!this.paused)await this.deliver({id:r.id,valueWire:TaliaValue.encode(value)});};run().catch(e=>{if(!this.closed&&!this.paused)this.deliver({id:r.id,error:String(e)}).catch(()=>{});});}}
 async poll(){if(this.closed||this.paused||!client.ready)return;let value;try{value=client.sample();}catch{return;}for(const [event,previous]of this.subscriptions){const key=client.incarnation+':'+value.revision+':'+value.evaluation;if(key!==previous){this.subscriptions.set(event,key);await this.deliver({event,valueWire:TaliaValue.encode(value)});}}}
 pause(){this.paused=true;client.setActive(false);}
 async resume(){this.paused=false;this.subscriptions.forEach((_,k)=>this.subscriptions.set(k,-1));await client.setActive(this.subscriptions.size>0);await client.tick();await this.poll();}
 close(){this.closed=true;this.subscriptions.clear();client.setActive(false);}
}
export {client};
