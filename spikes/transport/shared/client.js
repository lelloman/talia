// Shared candidate adapter inside QuickJS. Host owns HTTP endpoint and test namespace.
// No automatic action retries. API/defaults are experimental.
(() => {
  const clients=new Map();
  globalThis.__receive=raw=>{
    const message=JSON.parse(raw);
    clients.get(message.channel)?.receive(message);
  };
  globalThis.RemoteEngine=class {
    constructor(channel) {
      if(clients.has(channel)) throw Error('duplicate channel');
      this.channel=channel;this.epoch=0;this.next=0;this.pending=new Map();
      this.listeners=new Set();this.connected=false;this.snapshot=null;this.ignored=0;
      clients.set(channel,this);
    }
    rpc(op,args) {
      if(!this.connected) return Promise.reject(Error('disconnected'));
      if(this.pending.size>=32) return Promise.reject(Error('request budget'));
      const id=++this.next;
      return new Promise((resolve,reject)=>{
        this.pending.set(id,{resolve,reject,actionId:op==='action'?args.actionId:null});
        try {__send(JSON.stringify({channel:this.channel,epoch:this.epoch,id,op,args}));}
        catch(error){this.pending.delete(id);reject(error);}
      });
    }
    receive(message) {
      if(message.epoch!==this.epoch || !this.connected){this.ignored++;return;}
      const request=this.pending.get(message.id);
      if(!request){this.ignored++;return;}
      if(message.transportError){this.disconnect();return;}
      this.pending.delete(message.id);
      message.error!==undefined ? request.reject(Error(message.error)) : request.resolve(message.value);
    }
    disconnect() {
      this.connected=false;this.epoch++;this.next=0;this.watching=0;
      for(const request of this.pending.values()) {
        const error=Error(request.actionId?'outcome unknown':'disconnected');
        if(request.actionId)error.actionId=request.actionId;
        request.reject(error);
      }
      this.pending.clear();
    }
    apply(value,epoch) {
      if(!this.connected || epoch!==this.epoch)throw Error('disconnected');
      if(!value || !Number.isSafeInteger(value.revision) || !Number.isFinite(value.value))throw Error('invalid snapshot');
      if(this.snapshot && value.revision<this.snapshot.revision)throw Error('stale snapshot');
      if(!this.snapshot || value.revision>this.snapshot.revision) {
        this.snapshot=Object.freeze({...value});
        for(const listener of this.listeners)listener(this.snapshot);
      }
      return this.snapshot;
    }
    async connect() {
      this.disconnect();this.connected=true;this.snapshot=null;
      const value=await this.read('snapshot');this.watch();return value;
    }
    async read(tag='read') {
      const epoch=this.epoch;
      return this.apply(await this.rpc('read',{tag}),epoch);
    }
    subscribe(listener) {
      this.listeners.add(listener);
      if(this.snapshot)listener(this.snapshot);
      this.watch();
      return ()=>this.listeners.delete(listener);
    }
    async watch() {
      if(!this.connected || !this.listeners.size || this.watching===this.epoch)return;
      const epoch=this.epoch;this.watching=epoch;
      try {
        while(this.connected && this.epoch===epoch && this.listeners.size) {
          const value=await this.rpc('watch',{after:this.snapshot?.revision||0});
          // A slower poll can arrive after a newer direct read. Ignore its old snapshot.
          if(this.epoch===epoch && (!this.snapshot || value.revision>=this.snapshot.revision))this.apply(value,epoch);
        }
      } catch(error) {
        if(this.epoch===epoch)this.disconnect();
      } finally {if(this.watching===epoch)this.watching=0;}
    }
    write(actionId,value) {
      let resolve,reject,cancelled=false,dispatched=false;
      const epoch=this.epoch;
      const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});
      Promise.resolve().then(async()=>{
        if(cancelled)return;
        if(!this.connected || this.epoch!==epoch)throw Error('disconnected');
        dispatched=true;
        const outcome=await this.rpc('action',{actionId,value});
        if(cancelled)return;
        if(this.epoch!==epoch) {const e=Error('outcome unknown');e.actionId=actionId;throw e;}
        if(outcome.status==='completed' && (!this.snapshot || outcome.result.revision>=this.snapshot.revision))this.apply(outcome.result,epoch);
        resolve(outcome);
      }).catch(error=>{if(!cancelled)reject(error);});
      return {promise,cancel:()=>{
        cancelled=true;reject(Error('cancelled'));
        return dispatched && this.connected ? this.rpc('cancel',{actionId}) : Promise.resolve({status:'unknown'});
      }};
    }
    status(actionId){return this.rpc('status',{actionId});}
    test(command,key){return this.rpc('test',{command,key});}
  };
})();
