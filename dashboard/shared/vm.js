/* Runs inside the bounded guest, shared by native QuickJS and browser QuickJS. */
(() => {
  const copy=value=>{
    if(globalThis.TaliaValue)return TaliaValue.copy(value);
    const seen=new Set();let nodes=0;
    function check(v,depth=0){
      if(++nodes>20000||depth>64)throw Error('JSON complexity limit');
      if(v===null||typeof v==='string'||typeof v==='boolean'||typeof v==='number'&&Number.isFinite(v))return;
      if(!v||typeof v!=='object'||seen.has(v)||!Array.isArray(v)&&Object.getPrototypeOf(v)!==Object.prototype)throw Error('plain JSON required');
      seen.add(v);
      for(const d of Object.values(Object.getOwnPropertyDescriptors(v))){if(d.get||d.set)throw Error('accessor forbidden');check(d.value,depth+1);}
      seen.delete(v);
    }
    check(value);const text=JSON.stringify(value);if(text.length>131072)throw Error('state size limit');return JSON.parse(text);
  };
  let definition=null,root=null,sequence=0,epoch=0,paused=false,failure=null,dirty=false;
  const definitions=new Map(),functions=new Map(),pending=new Map(),subscriptions=new Map(),tasks=new Set();
  function defineVM(def){if(definition)throw Error('duplicate ViewModel');definition=def;}
  function check(task){if(failure)throw Error('dashboard stopped');if(paused||task.epoch!==epoch||task.cancelled)throw Error('cancelled');}
  function call(task,op,value){
    check(task);if(pending.size>=64)throw Error('pending call limit');
    const id=++sequence,raw=JSON.stringify({id,op,value:copy(value)});
    if(raw.length>32768)throw Error('request size limit');
    return new Promise((resolve,reject)=>{pending.set(id,{task,resolve,reject});__send(raw);});
  }
  function make(def,params,path){
    if(!def||typeof def.initial!=='function'||!def.actions||typeof def.actions!=='object')throw Error('invalid VM definition');
    return {def,params:copy(params),path,value:copy(def.initial(copy(params))),revision:0,children:new Map()};
  }
  function stop(error){if(!failure){failure=String(error);epoch++;for(const task of tasks)task.cancelled=true;for(const p of pending.values())p.reject(Error('cancelled'));pending.clear();subscriptions.clear();}}
  function invoke(cell,name,event){
    if(paused||failure)return Promise.reject(Error(paused?'paused':'dashboard stopped'));
    if(tasks.size>=64){stop('invocation limit');return Promise.reject(Error(failure));}
    const action=name==='@start'?cell.def.start:name==='@resume'?cell.def.resume:cell.def.actions[name];
    if(typeof action!=='function'){stop('unknown handler '+name);return Promise.reject(Error(failure));}
    const task={epoch,cancelled:false};tasks.add(task);const snapshots=new WeakMap();
    const ctx=Object.freeze({
      params:copy(cell.params),
      state(){check(task);const s={revision:cell.revision,value:copy(cell.value)};snapshots.set(s,cell.revision);return s;},
      commit(s,next){check(task);if(snapshots.get(s)!==cell.revision)throw Error('stale snapshot');const value=copy(next);check(task);if(snapshots.get(s)!==cell.revision)throw Error('stale snapshot');cell.value=value;cell.revision++;},
      read:key=>call(task,'read',key),write:(...args)=>call(task,'write',globalThis.TaliaValue?{...(args.length===2?{id:args[0]}:{}),wire:TaliaValue.encode(args.at(-1))}:args.at(-1)),run:id=>call(task,'run',id),
      alerts:Object.freeze({snapshot:()=>call(task,'alerts',{op:'snapshot',args:{}}),history:key=>call(task,'alerts',{op:'history',args:{key}}),acknowledge:alert=>call(task,'alerts',{op:'acknowledge',args:{key:alert.key,occurrence:alert.occurrence,expected:alert.revision}}),silence:(silence,expected)=>call(task,'alerts',{op:'silence_save',args:{silence,expected}})}),
      async subscribe(key,handler){
        if(typeof cell.def.actions[handler]!=='function')throw Error('unknown subscription handler');
        const id=await call(task,'subscribe',key);check(task);
        subscriptions.set(id,{cell,handler,key});
        return async()=>{subscriptions.delete(id);return call(task,'unsubscribe',id);};
      },
      instance(name,reference,params={}){
        check(task);if(typeof name!=='string'||!name||name.length>80)throw Error('instance name');
        if(!definitions.has(reference))throw Error('unknown VM reference');
        const signature=globalThis.TaliaValue?TaliaValue.stringify([reference,copy(params)]):JSON.stringify([reference,copy(params)]);
        let child=cell.children.get(name);
        if(child&&child.signature!==signature)throw Error('instance parameters changed; reload required');
        if(!child){if(cell.children.size>=64||cell.path.length>1024)throw Error('instance limit');child=make(definitions.get(reference),params,cell.path+'/'+name);child.signature=signature;cell.children.set(name,child);if(child.def.start)invoke(child,'@start',{target:child.path,value:null}).catch(()=>{});}
        return Object.freeze({state:()=>{check(task);return copy(child.value);},dispatch:(action,event={target:name,value:null})=>{check(task);return invoke(child,action,event);}});
      },
      fn(name,...args){check(task);const fn=functions.get(name);if(!fn)throw Error('unknown function reference');return fn(...copy(args));}
    });
    // Start on a microtask, so cancellation can skip work before it begins.
    return Promise.resolve().then(()=>{check(task);return action(ctx,copy(event));}).then(result=>{
      check(task);return result===undefined?null:copy(result);
    }).catch(error=>{
      if(!task.cancelled&&task.epoch===epoch&&!paused)stop(error);
      throw error;
    }).finally(()=>tasks.delete(task));
  }
  function dispatch(name,event){invoke(root,name,event).catch(()=>{});}
  function start(params={}){if(root)throw Error('already started');root=make(definition,params,'root');if(root.def.start)dispatch('@start',{target:'root',value:null});}
  function receive(raw){
    const msg=JSON.parse(raw);
    if(msg.valueWire)msg.value=TaliaValue.decode(msg.valueWire);
    if(msg.event){const sub=subscriptions.get(msg.event);if(sub&&!paused&&!failure)invoke(sub.cell,sub.handler,{target:msg.event,value:msg.value}).catch(()=>{});return;}
    const p=pending.get(msg.id);if(!p)return;pending.delete(msg.id);
    try{check(p.task);msg.error?p.reject(Error(String(msg.error))):p.resolve(copy(msg.value));}catch(e){p.reject(e);}
  }
  function pause(){
    paused=true;epoch++;for(const task of tasks)task.cancelled=true;
    for(const p of pending.values())p.reject(Error('cancelled'));pending.clear();
  }
  function resume(){
    if(failure)return;paused=false;
    // Host reinstalls snapshot subscriptions with the same IDs. No writes replay.
  }
  function snapshot(){return copy({state:root?.value??null,revision:root?.revision??0,dirty,failure,paused,subscriptions:[...subscriptions.keys()]});}
  Object.defineProperties(globalThis,{
    defineVM:{value:defineVM},
    defineVMReference:{value:(name,def)=>{if(root||definitions.has(name))throw Error('definition registry closed/duplicate');definitions.set(name,def);}},
    defineFunction:{value:(name,fn)=>{if(root||functions.has(name)||typeof fn!=='function')throw Error('function registry closed/duplicate');functions.set(name,fn);}},
    TaliaVM:{value:Object.freeze({start,dispatch,receive,pause,resume,snapshot,stop,reconcile(outcomes){const visit=cell=>{if(cell.def.resume)invoke(cell,'@resume',{target:cell.path,value:outcomes}).catch(()=>{});for(const child of cell.children.values())visit(child);};visit(root);},
      liveCommit(revision,value){if(paused||failure)throw Error("dashboard unavailable");if(root.revision!==revision)throw Error("stale snapshot");root.value=copy(value);root.revision++;return root.revision;},markDirty(){dirty=true;},replaceAction(name,fn){if(!root||typeof fn!=='function')throw Error('action');root.def.actions[name]=fn;dirty=true;}})}
  });
})();
