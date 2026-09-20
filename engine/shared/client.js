/* Trusted transport. Dashboard code receives only its declared resource capabilities. */
(()=>{
 class Connection {
  constructor({send,client,clock=()=>Date.now(),status=()=>{},snapshot=()=>{}}){Object.assign(this,{send,client,clock,onStatus:status,onSnapshot:snapshot});this.epoch=1;this.incarnation=null;this.revision=-1;this.values=new Map();this.actions=new Map();this.resources=new Set(['value']);this.subscribed=new Set();this.syncTail=Promise.resolve();this.active=false;this.ready=false;this.ever=false;this.busy=false;this.nextTry=0;this.onlineUntil=0;this.state='connecting...';this.stopped=false;}
  status(s){if(this.state!==s){this.state=s;this.onStatus(s);}}
  async call(op,args={},hello=false){const epoch=this.epoch,incarnation=this.incarnation;const r=await this.send({version:1,client:this.client,epoch,incarnation,op,args});
   if(this.stopped||epoch!==this.epoch)throw Error('obsolete request');if(r.version!==1||r.epoch!==epoch)throw Error('invalid response identity');
   if(!hello&&r.incarnation!==incarnation){this.ready=false;throw Error('server incarnation changed');}
   if(r.error){const e=Error(r.error);e.connection=false;throw e;}if(hello){if(typeof r.incarnation!=='string')throw Error('invalid server identity');this.incarnation=r.incarnation;this.revision=-1;this.subscribed.clear();}return r.value;
  }
  adopt(s){if(!s||!Number.isSafeInteger(s.revision)||!Array.isArray(s.values))throw Error('invalid snapshot');if(s.revision<this.revision)return;this.revision=s.revision;this.monitoringError=s.monitoringError??null;this.values=new Map(s.values.map(v=>[v.id,{...v,value:TaliaValue.decode(v.value)}]));this.onSnapshot(this.values);}
  failed(){this.ready=false;this.nextTry=this.clock()+1000;this.status('disconnected');}
  async sync(){
   for(const id of [...this.subscribed])if(!this.active||!this.resources.has(id)){await this.call('unsubscribe',{id});this.subscribed.delete(id);}
   if(this.active)for(const id of this.resources)if(!this.subscribed.has(id)){this.adopt(await this.call('subscribe',{id}));this.subscribed.add(id);}
  }
  async tick(){if(this.busy||this.stopped||this.clock()<this.nextTry)return;this.busy=true;
   try{if(!this.ready){this.status('connecting...');this.epoch++;this.adopt(await this.call('hello',{},true));await this.sync();
     for(const id of this.actions.keys())this.actions.set(id,await this.call('status',{actionId:id}));
     this.adopt(await this.call('snapshot'));this.ready=true;await this.sync();this.onlineUntil=this.ever?this.clock()+3000:0;this.ever=true;this.status(this.onlineUntil?'back online':'');
    }else{await this.sync();this.adopt(await this.call(this.active?'poll':'snapshot'));if(this.clock()>=this.onlineUntil)this.status('');}
   }catch(e){this.failed();}finally{this.busy=false;}
  }
  async setResources(resources){this.resources=new Set(resources);return this.setActive(this.active);}
  async setActive(active){this.active=active;this.syncTail=this.syncTail.catch(()=>{}).then(async()=>{if(this.ready&&!this.busy)try{await this.sync();}catch(e){if(e.connection!==false)this.failed();throw e;}});return this.syncTail;}
  sample(id='value'){const v=this.values.get(id);if(!v)throw Error('value unavailable: '+id);return v;}
  async read(id='value'){if(!this.ready)throw Error('disconnected');try{const v=await this.call('read',{id});return {...v,hasValue:v.hasValue??v.has_value,value:TaliaValue.decode(v.value)};}catch(e){if(e.connection!==false)this.failed();throw e;}}
  async action(op,args,actionId){if(!this.ready)throw Error('disconnected');if(this.actions.size>=128)throw Error('action tracking limit');this.actions.set(actionId,{status:'unknown',actionId});
   try{const r=await this.call(op,{...args,actionId});this.actions.set(actionId,r);if(r.status==='failed'){const e=Error(r.outcome?.error||'action failed');e.connection=false;throw e;}return r;}catch(e){if(e.connection!==false)this.failed();throw e;}
  }
  async write(value,actionId,id='value'){return this.action('write',{id,expected:this.sample(id).revision,value:TaliaValue.encode(value)},actionId);}
  async run(id,actionId){return this.action('run',{id},actionId);}
  close(){this.stopped=true;this.epoch++;}
 }
 Object.defineProperty(globalThis,'TaliaConnection',{value:Connection});
})();
