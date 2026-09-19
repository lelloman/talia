import '../shared/ui.js';
import {Renderer} from './renderer.js';import {Guest,EngineBridge} from './host.js';
const root=document.querySelector('#dashboard'),diagnostic=document.querySelector('#diagnostic');
let guest,bridge,tree,state,failure=null,refreshing=false,ready=false;
let config;
try{config=JSON.parse(localStorage.getItem('talia.client')||'{}');}catch{config={};}
if(!Number.isFinite(config.scale)||config.scale<0.25||config.scale>8)config.scale=1;
config.params={sidebar:config.params?.sidebar===true};
const scaleInput=document.querySelector('#dp-scale'),sidebarInput=document.querySelector('#sidebar');
scaleInput.value=config.scale;sidebarInput.checked=config.params.sidebar;
function saveConfig(){localStorage.setItem('talia.client',JSON.stringify(config));refresh();}
scaleInput.onchange=()=>{const scale=Number(scaleInput.value);if(Number.isFinite(scale)&&scale>=0.25&&scale<=8){config.scale=scale;saveConfig();}else scaleInput.value=config.scale;};
sidebarInput.onchange=()=>{config.params.sidebar=sidebarInput.checked;saveConfig();};
new ResizeObserver(()=>refresh()).observe(root);
const renderer=new Renderer(root,(name,event)=>command(`TaliaVM.dispatch(${JSON.stringify(name)},${JSON.stringify(event)});'ok';`));
function fail(error){if(failure)return;failure=String(error);bridge?.close();guest?.close();diagnostic.textContent='Dashboard stopped — '+failure;diagnostic.hidden=false;document.querySelector('#restart').hidden=false;root.inert=true;}
async function command(source){try{if(failure||document.hidden)return;await guest.eval(source);await refresh();}catch(e){fail(e);}}
async function refresh(){
 if(!ready||refreshing||failure||document.hidden)return;refreshing=true;
 try{const snapshot=JSON.parse(await guest.eval('JSON.stringify(TaliaVM.snapshot())'));if(snapshot.failure){fail(snapshot.failure);return;}state=snapshot.state;
  renderer.render(TaliaUI.resolve(tree,state,{width:root.clientWidth-24,scale:config.scale,params:config.params}),config.scale);window.dashboardReport={...snapshot,client:structuredClone(config),width:root.clientWidth-24,subscriptions:bridge.subscriptions.size,actions:[...bridge.actions.values()],externalError:bridge.externalError??null};
 }catch(e){fail(e);}finally{refreshing=false;}
}
async function start(){
 ready=false;bridge?.close();guest?.close();failure=null;diagnostic.hidden=true;root.inert=false;document.querySelector('#restart').hidden=true;
 const [ui,library,source]=await Promise.all(['examples/monitor.ui','shared/vm.js','examples/monitor.vm.js'].map(p=>fetch('/dashboard/'+p).then(r=>{if(!r.ok)throw Error('definition unavailable');return r.text();})));
 tree=TaliaUI.compile(ui);
 bridge=new EngineBridge(msg=>guest.eval(`TaliaVM.receive(${JSON.stringify(JSON.stringify(msg))});'ok';`).then(refresh),fail);
 guest=new Guest(requests=>bridge.requests(requests),fail);
 await guest.eval(library+'\n'+source+"\nTaliaVM.start({});'ok';",true);ready=true;await refresh();
}
document.querySelector('#restart').onclick=()=>start().catch(fail);
document.addEventListener('visibilitychange',async()=>{
 if(failure||!ready)return;
 try{if(document.hidden){bridge.pause();await guest.eval("TaliaVM.pause();'ok';");}else{await guest.eval("TaliaVM.resume();'ok';");await bridge.resume();await refresh();}}catch(e){fail(e);}
});
window.talia={renderer,refresh,command,get state(){return state;}};
setInterval(refresh,100);start().catch(fail);
