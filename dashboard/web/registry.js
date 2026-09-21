// Host-only identity: never pass credentials or ownership tokens into the guest worker.
const key='talia.registration.v1',slotKey='talia.slot.v1';
const uuid=()=>crypto.randomUUID();
const secret=()=>Array.from(crypto.getRandomValues(new Uint8Array(32)),v=>v.toString(16).padStart(2,'0')).join('');
const read=(storage,k)=>{try{return JSON.parse(storage.getItem(k)||'null');}catch{return null;}};
export class ClientRegistry {
 static async open(enabled){
  const registry=new ClientRegistry(enabled);
  await navigator.locks.request(key,async()=>{
   let registration=read(localStorage,key);
   if(!registration?.credential){registration={credential:secret(),name:'Web dashboard'};localStorage.setItem(key,JSON.stringify(registration));}
   registry.registration=registration;
  });
  let saved=read(sessionStorage,slotKey),owns=false;
  if(saved?.id)owns=await registry.lock(saved.id);
  if(!owns){saved={id:uuid(),owner:secret(),epoch:0,config:read(localStorage,'talia.client')||{}};await registry.lock(saved.id);}
  registry.slot=saved;registry.persist();
  addEventListener('pagehide',()=>registry.disconnect());
  return registry;
 }
 constructor(enabled){Object.assign(this,{enabled,busy:false,connected:false,sequence:0,report:null,last:0,error:null});}
 lock(id){return new Promise(resolve=>{navigator.locks.request('talia.slot.'+id,{ifAvailable:true},lock=>{resolve(!!lock);if(lock)return new Promise(release=>{this.release=release;});}).catch(()=>resolve(false));});}
 persist(){sessionStorage.setItem(slotKey,JSON.stringify(this.slot));}
 get config(){return this.slot.config;}
 saveConfig(config){this.slot.config=config;this.persist();}
 replace(report){this.previous=this.slot.live||null;this.slot.live=uuid();this.report=report;this.connected=false;this.sequence=0;this.slot.epoch++;this.persist();this.last=0;}
 setReport(report){this.report=report;}
 async send(body,keepalive=false){const r=await fetch('/clients',{method:'POST',headers:{'Content-Type':'application/json',Authorization:'Bearer '+this.registration.credential},body:JSON.stringify(body),signal:AbortSignal.timeout(5000),keepalive});if(!r.ok)throw Error('HTTP '+r.status);const reply=await r.json();if(reply.error)throw Object.assign(Error(reply.error),{code:reply.error});return reply;}
 address(){return {slot:this.slot.id,owner:this.slot.owner,live:this.slot.live,epoch:this.slot.epoch};}
 async tick(force=false){
  if(!this.enabled||!this.report||this.busy||(!force&&Date.now()-this.last<3000))return;
  this.busy=true;this.last=Date.now();const live=this.slot.live;
  try{
   const enrolled=await this.send({op:'register',name:this.registration.name,platform:'web'});
   this.registration.clientId=enrolled.value.clientId;this.registration.name=enrolled.value.name;localStorage.setItem(key,JSON.stringify(this.registration));
   if(live!==this.slot.live)return;
   if(!this.connected){this.slot.epoch++;this.persist();const status=await this.send({op:'status'});if(live!==this.slot.live)return;const previous=status.value.slots.find(s=>s.slotId===this.slot.id)?.liveInstanceId??null;await this.send({op:'connect',...this.address(),previous,report:this.report});if(live!==this.slot.live)return;this.connected=true;this.sequence=0;this.previous=null;}
   await this.send({op:'report',...this.address(),sequence:++this.sequence,report:this.report});if(live===this.slot.live)this.error=null;
  }catch(e){if(live===this.slot.live){this.connected=false;this.error=String(e);}}finally{this.busy=false;}
 }
 disconnect(){if(!this.enabled||!this.report)return;this.send({op:'disconnect',...this.address(),sequence:++this.sequence},true).catch(()=>{});this.connected=false;}
 async enroll(){const r=await this.send({op:'register',name:this.registration.name,platform:'web'});this.registration.clientId=r.value.clientId;this.registration.name=r.value.name;localStorage.setItem(key,JSON.stringify(this.registration));}
 async assignment(){await this.enroll();return (await this.send({op:'openSlot',slot:this.slot.id,owner:this.slot.owner})).value;}
 async prepare(){await this.assignment();return (await this.send({op:'delivery',slot:this.slot.id,owner:this.slot.owner})).value;}
 async confirm(revision){await this.send({op:'confirmDelivery',slot:this.slot.id,owner:this.slot.owner,revision});}
 async select(dashboardId,params={},presentation={}){const current=window.taliaOidc?(await this.send({op:'selectionState',slot:this.slot.id,owner:this.slot.owner})).value:await this.assignment();return (await this.send({op:'select',slot:this.slot.id,owner:this.slot.owner,expected:current.revision,assignment:{dashboardId,params,presentation}})).value;}
 cacheKey(){return 'talia.baseline.'+this.registration.clientId+'.'+this.slot.id;}
 cached(){return read(localStorage,this.cacheKey());}
 cache(delivery){localStorage.setItem(this.cacheKey(),JSON.stringify(delivery));}
 async rename(name){await this.send({op:'rename',name});this.registration.name=name;localStorage.setItem(key,JSON.stringify(this.registration));}
 publicStatus(){return {clientId:this.registration.clientId??null,name:this.registration.name,slotId:this.slot.id,liveInstanceId:this.slot.live??null,connected:this.connected,error:this.error};}
}
