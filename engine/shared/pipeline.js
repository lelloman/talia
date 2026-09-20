/* Bounded guest capability bridge. Values/state remain lossless across host calls. */
(()=>{
 let next=0;const pending=new Map(),queue=[];let state=TaliaValue.decode(input.state);
 const call=(op,args)=>new Promise((resolve,reject)=>{if(pending.size>=64)throw Error('pending limit');const id=++next;pending.set(id,{resolve,reject});queue.push({id,op,...args});});
 const ctx=Object.freeze({params:TaliaValue.decode(input.params),now:()=>input.now,
  get state(){return state},set state(v){state=TaliaValue.copy(v)},
  read:alias=>call('read',{alias}),source:(alias,request)=>call('source',{alias,request}),
  publish:(alias,value)=>call('publish',{alias,value:TaliaValue.encode(value)}),
  commit:()=>call('commit',{state:TaliaValue.encode(state)}),sleep:ms=>call('sleep',{ms})});
 globalThis.invocation={take:()=>JSON.stringify(queue.splice(0)),result:null,receive(m){const p=pending.get(m.id);if(!p)return;pending.delete(m.id);m.error?p.reject(Error(m.error)):p.resolve(TaliaValue.decode(m.value));}};
 Promise.resolve().then(()=>definition.run(ctx)).then(value=>{invocation.result={ok:true,value:TaliaValue.encode(value),state:TaliaValue.encode(state)}},e=>{invocation.result={ok:false,error:String(e)}});
})();
