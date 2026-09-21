// Trusted host transport. No credentials are passed to the dashboard worker.
export async function alertRequest(op,args={}){
 const token=sessionStorage.getItem('talia.alertCredential');if(!window.taliaOidc&&!token)throw Error('Alert access is not configured');
 if(!['snapshot','history','config','audit'].includes(op)&&!args.requestId)args={...args,requestId:crypto.randomUUID()};
 const response=await fetch('/alerts',{method:'POST',headers:{'Content-Type':'application/json',...(!window.taliaOidc?{Authorization:'Bearer '+token}:{})},body:JSON.stringify({op,args}),signal:AbortSignal.timeout(5000)});
 if(!response.ok)throw Error('Alert connection failed');const result=await response.json();if(result.error)throw Error(result.error);return result;
}
const panel=document.querySelector('#alerts-panel');
if(panel){
 const access=document.querySelector('#alert-access'),status=document.querySelector('#alert-status'),list=document.querySelector('#alert-list');
 document.querySelector('#alert-connect').onclick=()=>{if(access.value){sessionStorage.setItem('talia.alertCredential',access.value);access.value='';}refresh();};
 const node=(tag,text)=>{const n=document.createElement(tag);n.textContent=text;return n;};
 const button=(text,action)=>{const b=node('button',text);b.onclick=async()=>{b.disabled=true;try{await action();await refresh();}catch(e){status.textContent=String(e);b.disabled=false;}};return b;};
 async function refresh(){if(!panel.open||document.hidden)return;
  try{const state=await alertRequest('snapshot');list.replaceChildren();status.textContent='';
   for(const a of state.alerts){const item=node('article','');item.append(node('h3',a.key),node('p',`${a.severity} · ${a.active?'Active':'Resolved'}${a.acknowledgement?' · Acknowledged by '+a.acknowledgement.actor:''}`),node('p',a.message));
    if(a.active&&!a.acknowledgement)item.append(button('Acknowledge',()=>alertRequest('acknowledge',{key:a.key,occurrence:a.occurrence,expected:a.revision})));
    item.append(button('Silence for 1 hour',()=>alertRequest('silence_save',{expected:0,silence:{id:crypto.randomUUID(),version:1,key:a.key,until:state.now+3600000,reason:'Dashboard maintenance'}})));
    const history=node('div','');item.append(button('History',async()=>{const r=await alertRequest('history',{key:a.key});history.replaceChildren(...r.occurrences.map(h=>node('p',`Occurrence ${h.occurrence}: ${h.active?'active':'resolved'} · ${h.message}`)));}),history);
    for(const d of state.deliveries.filter(d=>d.key===a.key&&(d.error||d.status==='failed')))item.append(node('p',`${d.destination}: ${d.status}${d.error?' — '+d.error:''}`));list.append(item);
   }
   for(const s of state.silences.filter(s=>s.until>state.now)){const item=node('p',`Silenced: ${s.key||JSON.stringify(s.labels)} until ${new Date(s.until).toLocaleString()}`);item.append(button('End silence',()=>alertRequest('silence_save',{expected:s.version,silence:{...s,version:s.version+1,until:0}})));list.append(item);}
   for(const e of state.evaluations.filter(e=>e.error))list.append(node('p',`${e.id}: ${e.error}`));
   if(!state.alerts.length)list.append(node('p','No alerts'));
  }catch(e){status.textContent=String(e);}
 }
 panel.addEventListener('toggle',refresh);document.querySelector('#alert-refresh').onclick=refresh;setInterval(refresh,3000);
}
