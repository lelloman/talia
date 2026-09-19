// Saved fixture definition, shared by both native hosts. No platform APIs.
globalThis.vm = {state:{dirty:false,ticks:0}, action:()=>2};
globalThis.lastError=null;
globalThis.writeValue = value => engine.write(value).catch(e=>{lastError=String(e);});
