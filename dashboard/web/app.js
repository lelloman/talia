import '../shared/ui.js';
import '../../engine/shared/value.js';
import {DurableBridge,client as engineClient} from './durable.js';
import {ClientRegistry} from './registry.js';
const durable=new URLSearchParams(location.search).get('engine')!=='legacy';
import {Renderer} from './renderer.js';import {Guest,EngineBridge} from './host.js';
const root=document.querySelector('#dashboard'),diagnostic=document.querySelector('#diagnostic');
let uiRevision=0;
let guest,bridge,loaded,state,failure=null,refreshing=false,ready=false,generation=0,checking=false;
const registry=await ClientRegistry.open(durable);
let editRevision=0;
let config=registry.config;
if(!Number.isFinite(config?.scale)||config.scale<0.25||config.scale>8)config={...config,scale:1};
if(!/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(config.dashboardId||''))config.dashboardId='monitor';
config.params={sidebar:config.params?.sidebar===true};
const scaleInput=document.querySelector('#dp-scale'),sidebarInput=document.querySelector('#sidebar'),update=document.querySelector('#update'),dirty=document.querySelector('#dirty');
scaleInput.value=config.scale;sidebarInput.checked=config.params.sidebar;
function saveConfig(){registry.saveConfig(config);refresh();}
scaleInput.onchange=()=>{const scale=Number(scaleInput.value);if(Number.isFinite(scale)&&scale>=0.25&&scale<=8){config.scale=scale;saveConfig();}else scaleInput.value=config.scale;};
sidebarInput.onchange=()=>{config.params.sidebar=sidebarInput.checked;saveConfig();};
new ResizeObserver(()=>refresh()).observe(root);
const renderer=new Renderer(root,(name,event)=>command(`TaliaVM.dispatch(${JSON.stringify(name)},${JSON.stringify(event)});'ok';`));
function reportRegistry(){if(!loaded)return;if(window.dashboardReport)window.dashboardReport.registration=registry.publicStatus();registry.setReport({dashboardId:loaded.id,packageRevision:loaded.revision,lifecycle:failure?'failed':document.hidden||!ready?'paused':'active',foreground:!document.hidden,dirty:editRevision>0||window.dashboardReport?.dirty===true,editRevision,updateAvailable:!update.hidden});registry.tick();}
function fail(error){if(failure)return;failure=String(error);bridge?.close();guest?.close();diagnostic.textContent='Dashboard stopped — '+failure;diagnostic.hidden=false;document.querySelector('#restart').hidden=false;root.inert=true;window.dashboardReport={...window.dashboardReport,failure,subscriptions:0};
 reportRegistry();registry.tick(true);
 if(config.failureSignals===true)window.dispatchEvent(new CustomEvent('talia-dashboard-failure',{detail:{instance:loaded?.id,diagnostic:failure}}));
}
async function command(source){uiRevision++;try{if(failure||document.hidden||!ready)return;await guest.eval(source);await refresh();}catch(e){fail(e);}}
async function refresh(){
 if(!ready||refreshing||failure||document.hidden)return;refreshing=true;const stamp=generation,revision=uiRevision;
 try{const snapshot=TaliaValue.parse(await guest.eval('TaliaValue.stringify(TaliaVM.snapshot())'));if(stamp!==generation||revision!==uiRevision)return;if(snapshot.failure){fail(snapshot.failure);return;}state=snapshot.state;
  renderer.render(TaliaUI.resolve(loaded.ui,state,{definitions:loaded.definitions,width:Math.max(0,root.clientWidth-24),scale:config.scale,params:{...loaded.params,...config.params}}),config.scale);
  dirty.hidden=!snapshot.dirty;window.dashboardReport={...snapshot,stateWire:TaliaValue.encode(snapshot.state),registration:registry.publicStatus(),editRevision,connection:durable?engineClient.state:null,revision:loaded.revision,client:structuredClone(config),width:root.clientWidth-24,subscriptions:bridge.subscriptions.size,actions:[...bridge.actions.values()],actionIds:[...bridge.actions.keys()],nextActionId:bridge.nextActionId,externalError:bridge.externalError??null};reportRegistry();
 }catch(e){if(stamp===generation)fail(e);}finally{refreshing=false;}
}
async function fetchPackage(id=config.dashboardId){
 const r=await fetch('/dashboard/package.json?dashboard='+encodeURIComponent(id),{cache:'no-store',signal:AbortSignal.timeout(5000)});if(!r.ok)throw Error('definition unavailable');const text=await r.text();if(text.length>262144)throw Error('package size limit');const pkg=TaliaUI.validatePackage(JSON.parse(text));if(pkg.id!==id)throw Error('dashboard assignment mismatch');return pkg;
}
async function checkUpdates(){
 if(checking||!loaded||document.hidden)return;checking=true;
 try{const latest=await fetchPackage();if(latest.id===loaded.id)update.hidden=latest.revision===loaded.revision;}catch{/* A failed check must not stop the loaded dashboard. */}finally{checking=false;}
}
async function start(){
 const stamp=++generation;ready=false;bridge?.close();guest?.close();failure=null;diagnostic.hidden=true;root.inert=false;document.querySelector('#restart').hidden=true;
 let pkg;try{pkg=await fetchPackage();}catch(e){const saved=localStorage.getItem('talia.saved.'+config.dashboardId);if(!saved)throw e;pkg=TaliaUI.validatePackage(JSON.parse(saved));}
 if(pkg.id!==config.dashboardId)throw Error('dashboard assignment mismatch');
 const r=await fetch('/dashboard/shared/vm.js');if(!r.ok)throw Error('VM support unavailable');const library=await r.text();if(stamp!==generation)return;
 loaded=structuredClone(pkg);state=null;update.hidden=true;editRevision=0;if(window.dashboardReport)window.dashboardReport.dirty=false;registry.replace({dashboardId:loaded.id,packageRevision:loaded.revision,lifecycle:'paused',foreground:!document.hidden,dirty:false,editRevision:0,updateAvailable:false});
 const codec=await (await fetch('/engine/shared/value.js')).text();
 let localGuest;
 const localBridge=new (durable?DurableBridge:EngineBridge)(msg=>localGuest.eval(`TaliaVM.receive(${JSON.stringify(JSON.stringify(msg))});'ok';`).then(refresh),e=>{if(stamp===generation)fail(e);},loaded.grants);
 localGuest=new Guest(requests=>localBridge.requests(requests),e=>{if(stamp===generation)fail(e);});bridge=localBridge;guest=localGuest;
 await guest.eval(codec+'\n'+library+'\n'+loaded.viewModel+'\nTaliaVM.start('+JSON.stringify({...loaded.params,...config.params})+');\'ok\';',true);
 if(stamp!==generation){localBridge.close();localGuest.close();return;}
 localStorage.setItem('talia.saved.'+loaded.id,JSON.stringify(loaded));ready=true;if(document.hidden){bridge.pause();await guest.eval("TaliaVM.pause();'ok';");}else await refresh();
}
document.querySelector('#restart').onclick=()=>start().catch(fail);
document.querySelector('#reload').onclick=()=>start().catch(fail);
document.addEventListener('visibilitychange',async()=>{
 reportRegistry();registry.tick(true);
 if(failure||!ready)return;
 try{if(document.hidden){bridge.pause();await guest.eval("TaliaVM.pause();'ok';");window.dashboardReport={...window.dashboardReport,paused:true,subscriptions:0};}else{await guest.eval("TaliaVM.resume();'ok';");await bridge.resume();await guest.eval("TaliaVM.reconcile("+JSON.stringify([...bridge.actions.values()])+");'ok';");await refresh();await checkUpdates();}}catch(e){fail(e);}
});
window.talia={renameClient:name=>registry.rename(name),registration:()=>registry.publicStatus(),renderer,refresh,command,checkUpdates,reload:start,async selectDashboard(id){if(!/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(id))throw Error('invalid dashboard ID');config.dashboardId=id;registry.saveConfig(config);try{await start();}catch(e){fail(e);throw e;}},async live(source){if(failure||document.hidden||!ready)return;editRevision++;reportRegistry();await command('TaliaVM.markDirty();\n'+source+"\n;'ok';");},get state(){return state;}};
setInterval(()=>{reportRegistry();},500);setInterval(refresh,100);setInterval(checkUpdates,2000);start().catch(fail);
