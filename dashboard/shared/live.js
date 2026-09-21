/* Isolated invocation guest. Only the host can reach the loaded VM or engine. */
(()=>{
 const send=__send,copy=TaliaValue.copy;let sequence=0,done=null;const pending=new Map(),snapshots=new WeakMap();let current=copy(liveInitial);
 function call(op,value){if(pending.size>=32)throw Error('pending call limit');const id=++sequence;return new Promise((resolve,reject)=>{pending.set(id,{resolve,reject});send(JSON.stringify({id,op,value}));});}
 const ctx=Object.freeze({
  state(){const s=copy(current);snapshots.set(s,s.revision);return s;},
  async commit(snapshot,next){if(snapshots.get(snapshot)!==current.revision)throw Error('stale snapshot');const value=copy(next);const revision=await call('commit',{revision:snapshot.revision,value:TaliaValue.encode(value)});current={revision,value};},
  read:id=>call('read',id),write:(id,value)=>call('write',{id,wire:TaliaValue.encode(value)}),run:id=>call('run',id)
 });
 Object.defineProperty(globalThis,'TaliaLive',{value:Object.freeze({
  start(source){const fn=new (Object.getPrototypeOf(async function(){}).constructor)('ctx',source);Promise.resolve().then(()=>fn(ctx)).then(value=>{done={complete:true,value:TaliaValue.encode(value)};}).catch(()=>{done={complete:true,error:'validation_failed'};});},
  receive(raw){const r=JSON.parse(raw),p=pending.get(r.id);if(!p)return;pending.delete(r.id);r.error?p.reject(Error(r.error)):p.resolve(TaliaValue.decode(r.value));},
  snapshot(){return JSON.stringify(done);}
 })});
})();
