(()=>{
 let state=TaliaValue.decode(input.state),next=0;const actions=new Set(),pending=new Map(),queue=[];
 const samples=Object.fromEntries(Object.entries(input.samples).map(([k,v])=>[k,TaliaValue.decode(v)]));
 const ctx=Object.freeze({params:TaliaValue.decode(input.params),changed:Object.freeze(input.changed),reason:input.reason,now:()=>input.now,
  get state(){return state},set state(v){state=TaliaValue.copy(v)},
  async read(alias){if(!Object.hasOwn(samples,alias))throw Error('input not granted');return TaliaValue.copy(samples[alias]);},
  async trigger(alias){if(!input.actions.includes(alias))throw Error('action not granted');if(actions.size>=32)throw Error('action limit');actions.add(alias);},
  sleep:ms=>new Promise((resolve,reject)=>{if(pending.size>=64)throw Error('pending limit');const id=++next;pending.set(id,{resolve,reject});queue.push({id,ms});})});
 globalThis.invocation={result:null,take:()=>JSON.stringify(queue.splice(0)),receive(m){const p=pending.get(m.id);if(p){pending.delete(m.id);p.resolve();}}};
 Promise.resolve().then(()=>definition.evaluate(ctx)).then(()=>{invocation.result={ok:true,state:TaliaValue.encode(state),actions:[...actions]}},error=>{invocation.result={ok:false,error:String(error)}});
})();
