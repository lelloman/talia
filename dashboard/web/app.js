import '../shared/ui.js';
import {Renderer} from './renderer.js';import {Guest,EngineBridge} from './host.js';
const root=document.querySelector('#dashboard'),diagnostic=document.querySelector('#diagnostic');
let guest,bridge,loaded,state,failure=null,refreshing=false,ready=false,generation=0,checking=false;
let config;try{config=JSON.parse(localStorage.getItem('talia.client')||'{}');}catch{config={};}
if(!Number.isFinite(config?.scale)||config.scale<0.25||config.scale>8)config={...config,scale:1};
config.params={sidebar:config.params?.sidebar===true};
const scaleInput=document.querySelector('#dp-scale'),sidebarInput=document.querySelector('#sidebar'),update=document.querySelector('#update'),dirty=document.querySelector('#dirty');
scaleInput.value=config.scale;sidebarInput.checked=config.params.sidebar;
function saveConfig(){localStorage.setItem('talia.client',JSON.stringify(config));refresh();}
scaleInput.onchange=()=>{const scale=Number(scaleInput.value);if(Number.isFinite(scale)&&scale>=0.25&&scale<=8){config.scale=scale;saveConfig();}else scaleInput.value=config.scale;};
sidebarInput.onchange=()=>{config.params.sidebar=sidebarInput.checked;saveConfig();};
new ResizeObserver(()=>refresh()).observe(root);
const renderer=new Renderer(root,(name,event)=>command(`TaliaVM.dispatch(${JSON.stringify(name)},${JSON.stringify(event)});'ok';`));
function fail(error){if(failure)return;failure=String(error);bridge?.close();guest?.close();diagnostic.textContent='Dashboard stopped — '+failure;diagnostic.hidden=false;document.querySelector('#restart').hidden=false;root.inert=true;window.dashboardReport={...window.dashboardReport,failure};
 if(config.failureSignals===true)window.dispatchEvent(new CustomEvent('talia-dashboard-failure',{detail:{instance:loaded?.id,diagnostic:failure}}));
}
async function command(source){try{if(failure||document.hidden||!ready)return;await guest.eval(source);await refresh();}catch(e){fail(e);}}
async function refresh(){
 if(!ready||refreshing||failure||document.hidden)return;refreshing=true;const stamp=generation;
 try{const snapshot=JSON.parse(await guest.eval('JSON.stringify(TaliaVM.snapshot())'));if(stamp!==generation)return;if(snapshot.failure){fail(snapshot.failure);return;}state=snapshot.state;
  renderer.render(TaliaUI.resolve(loaded.ui,state,{definitions:loaded.definitions,width:Math.max(0,root.clientWidth-24),scale:config.scale,params:{...loaded.params,...config.params}}),config.scale);
  dirty.hidden=!snapshot.dirty;window.dashboardReport={...snapshot,revision:loaded.revision,client:structuredClone(config),width:root.clientWidth-24,subscriptions:bridge.subscriptions.size,actions:[...bridge.actions.values()],externalError:bridge.externalError??null};
 }catch(e){if(stamp===generation)fail(e);}finally{refreshing=false;}
}
async function fetchPackage(){
 const r=await fetch('/dashboard/package.json',{cache:'no-store',signal:AbortSignal.timeout(5000)});if(!r.ok)throw Error('definition unavailable');const text=await r.text();if(text.length>262144)throw Error('package size limit');return TaliaUI.validatePackage(JSON.parse(text));
}
async function checkUpdates(){
 if(checking||!loaded||document.hidden)return;checking=true;
 try{const latest=await fetchPackage();update.hidden=latest.revision===loaded.revision;}catch{/* A failed check must not stop the loaded dashboard. */}finally{checking=false;}
}
async function start(){
 const stamp=++generation;ready=false;bridge?.close();guest?.close();failure=null;diagnostic.hidden=true;root.inert=false;document.querySelector('#restart').hidden=true;
 let pkg;try{pkg=await fetchPackage();}catch(e){const saved=localStorage.getItem('talia.saved');if(!saved)throw e;pkg=TaliaUI.validatePackage(JSON.parse(saved));}
 const r=await fetch('/dashboard/shared/vm.js');if(!r.ok)throw Error('VM support unavailable');const library=await r.text();if(stamp!==generation)return;
 loaded=structuredClone(pkg);state=null;update.hidden=true;
 let localGuest;
 const localBridge=new EngineBridge(msg=>localGuest.eval(`TaliaVM.receive(${JSON.stringify(JSON.stringify(msg))});'ok';`).then(refresh),e=>{if(stamp===generation)fail(e);});
 localGuest=new Guest(requests=>localBridge.requests(requests),e=>{if(stamp===generation)fail(e);});bridge=localBridge;guest=localGuest;
 await guest.eval(library+'\n'+loaded.viewModel+'\nTaliaVM.start('+JSON.stringify({...loaded.params,...config.params})+');\'ok\';',true);
 if(stamp!==generation){localBridge.close();localGuest.close();return;}
 localStorage.setItem('talia.saved',JSON.stringify(loaded));ready=true;if(document.hidden){bridge.pause();await guest.eval("TaliaVM.pause();'ok';");}else await refresh();
}
document.querySelector('#restart').onclick=()=>start().catch(fail);
document.querySelector('#reload').onclick=()=>start().catch(fail);
document.addEventListener('visibilitychange',async()=>{
 if(failure||!ready)return;
 try{if(document.hidden){bridge.pause();await guest.eval("TaliaVM.pause();'ok';");}else{await guest.eval("TaliaVM.resume();'ok';");await bridge.resume();await refresh();await checkUpdates();}}catch(e){fail(e);}
});
window.talia={renderer,refresh,command,checkUpdates,reload:start,async live(source){await command('TaliaVM.markDirty();\n'+source+"\n;'ok';");},get state(){return state;}};
setInterval(refresh,100);setInterval(checkUpdates,2000);start().catch(fail);
