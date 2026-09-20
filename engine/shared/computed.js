/* Isolated invocation support. Host validates all requests and owns revisions. */
(()=>{
 let next=0;const pending=new Map(),queue=[];
 const call=(op,arg)=>new Promise((resolve,reject)=>{if(pending.size>=64)throw Error('pending limit');const id=++next;pending.set(id,{resolve,reject});queue.push({id,op,arg:TaliaValue.encode(arg)});});
 let state=TaliaValue.decode(input.state);
 const ctx=Object.freeze({params:TaliaValue.decode(input.params),changed:Object.freeze(input.changed),now:()=>input.now,
  get state(){return state;},set state(v){state=TaliaValue.copy(v);},
  read:id=>call('read',id),write:(id,value)=>call('write',{id,value}),sleep:ms=>call('sleep',ms),
  commit:()=>call('commit',state)
 });
 globalThis.invocation={take:()=>JSON.stringify(queue.splice(0)),receive(raw){const m=JSON.parse(raw),p=pending.get(m.id);if(!p)return;pending.delete(m.id);m.error?p.reject(Error(m.error)):p.resolve(TaliaValue.decode(m.value));},result:null};
 Promise.resolve().then(()=>{const fn=input.mode==='get'?definition.get:definition.set;if(typeof fn!=='function')throw Error('operation unavailable');return fn(ctx,TaliaValue.decode(input.argument));}).then(value=>{invocation.result={ok:true,value:TaliaValue.encode(value),state:TaliaValue.encode(state)};},error=>{invocation.result={ok:false,error:String(error)};});
})();
