/* Trusted transport, shared by browser and native UI host. No dashboard code runs here. */
(()=>{
 class Connection {
  constructor({send,client,clock=()=>Date.now(),status=()=>{},snapshot=()=>{}}){Object.assign(this,{send,client,clock,onStatus:status,onSnapshot:snapshot});this.epoch=1;this.incarnation=null;this.revision=-1;this.values=new Map();this.actions=new Map();this.active=false;this.ready=false;this.ever=false;this.busy=false;this.nextTry=0;this.onlineUntil=0;this.state='connecting...';this.stopped=false;}
  status(s){if(this.state!==s){this.state=s;this.onStatus(s);}}
  async call(op,args={},hello=false){
   const epoch=this.epoch,incarnation=this.incarnation;
   const r=await this.send({version:1,client:this.client,epoch,incarnation,op,args});
   if(this.stopped||epoch!==this.epoch)throw Error('obsolete request');
   if(r.version!==1||r.epoch!==epoch)throw Error('invalid response identity');
   if(!hello&&r.incarnation!==incarnation){this.ready=false;throw Error('server incarnation changed');}
   if(r.error){const e=Error(r.error);e.connection=false;throw e;}
   if(hello){if(typeof r.incarnation!=='string')throw Error('invalid server identity');this.incarnation=r.incarnation;this.revision=-1;}
   return r.value;
  }
  adopt(s){if(!s||!Number.isSafeInteger(s.revision)||!Array.isArray(s.values))throw Error('invalid snapshot');if(s.revision<this.revision)return;this.revision=s.revision;this.values=new Map(s.values.map(v=>[v.id,{...v,value:TaliaValue.decode(v.value)}]));this.onSnapshot(this.values);}
  failed(){this.ready=false;this.nextTry=this.clock()+1000;this.status('disconnected');}
  async tick(){if(this.busy||this.stopped||this.clock()<this.nextTry)return;this.busy=true;
   try{
    if(!this.ready){this.status('connecting...');this.epoch++;const s=await this.call('hello',{},true);this.adopt(s);
     if(this.active)this.adopt(await this.call('subscribe',{id:'value'}));
     for(const id of this.actions.keys())this.actions.set(id,await this.call('status',{actionId:id}));
     this.adopt(await this.call('snapshot'));this.ready=true;this.onlineUntil=this.ever?this.clock()+3000:0;this.ever=true;this.status(this.onlineUntil?'back online':'');
    }else{this.adopt(await this.call(this.active?'poll':'snapshot'));if(this.clock()>=this.onlineUntil)this.status('');}
   }catch(e){this.failed();}finally{this.busy=false;}
  }
  async setActive(active){this.active=active;if(this.ready)try{const s=await this.call(active?'subscribe':'unsubscribe',{id:'value'});if(active)this.adopt(s);}catch{this.failed();}}
  sample(){const v=this.values.get('value');if(!v)throw Error('value unavailable');return v;}
  async read(){if(!this.ready)throw Error('disconnected');try{const v=await this.call('read',{id:'value'});return {...v,value:TaliaValue.decode(v.value)};}catch(e){if(e.connection!==false)this.failed();throw e;}}
  async write(value,actionId){if(!this.ready)throw Error('disconnected');const expected=this.sample().revision;this.actions.set(actionId,{status:'unknown',actionId});
   try{const r=await this.call('write',{id:'value',expected,value:TaliaValue.encode(value),actionId});this.actions.set(actionId,r);if(r.status==='failed'){const e=Error(r.outcome?.error||'write failed');e.connection=false;throw e;}return r;}catch(e){if(e.connection!==false)this.failed();throw e;}
  }
  close(){this.stopped=true;this.epoch++;}
 }
 Object.defineProperty(globalThis,'TaliaConnection',{value:Connection});
})();
