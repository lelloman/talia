import '../shared/ui.js';
import {Renderer} from './renderer.js';import {Guest,EngineBridge} from './host.js';
const root=document.querySelector('#dashboard'),diagnostic=document.querySelector('#diagnostic');
let guest,bridge,tree,state,failure=null,refreshing=false;
const renderer=new Renderer(root,(name,event)=>command(`TaliaVM.dispatch(${JSON.stringify(name)},${JSON.stringify(event)});'ok';`));
function fail(error){if(failure)return;failure=String(error);bridge?.close();guest?.close();diagnostic.textContent='Dashboard stopped — '+failure;diagnostic.hidden=false;document.querySelector('#restart').hidden=false;root.inert=true;}
async function command(source){try{if(failure||document.hidden)return;await guest.eval(source);await refresh();}catch(e){fail(e);}}
async function refresh(){
 if(refreshing||failure||document.hidden)return;refreshing=true;
 try{const snapshot=JSON.parse(await guest.eval('JSON.stringify(TaliaVM.snapshot())'));if(snapshot.failure){fail(snapshot.failure);return;}state=snapshot.state;
  renderer.render(TaliaUI.resolve(tree,state),1);window.dashboardReport={...snapshot,subscriptions:bridge.subscriptions.size,actions:[...bridge.actions.values()],externalError:bridge.externalError??null};
 }catch(e){fail(e);}finally{refreshing=false;}
}
async function start(){
 bridge?.close();guest?.close();failure=null;diagnostic.hidden=true;root.inert=false;document.querySelector('#restart').hidden=true;
 const [ui,library,source]=await Promise.all(['examples/monitor.ui','shared/vm.js','examples/monitor.vm.js'].map(p=>fetch('/dashboard/'+p).then(r=>{if(!r.ok)throw Error('definition unavailable');return r.text();})));
 tree=TaliaUI.compile(ui);
 bridge=new EngineBridge(msg=>guest.eval(`TaliaVM.receive(${JSON.stringify(JSON.stringify(msg))});'ok';`).then(refresh),fail);
 guest=new Guest(requests=>bridge.requests(requests),fail);
 await guest.eval(library+'\n'+source+"\nTaliaVM.start({});'ok';",true);await refresh();
}
document.querySelector('#restart').onclick=()=>start().catch(fail);
document.addEventListener('visibilitychange',async()=>{
 if(failure)return;
 try{if(document.hidden){bridge.pause();await guest.eval("TaliaVM.pause();'ok';");}else{await guest.eval("TaliaVM.resume();'ok';");await bridge.resume();await refresh();}}catch(e){fail(e);}
});
window.talia={renderer,refresh,command,get state(){return state;}};
setInterval(refresh,100);start().catch(fail);
