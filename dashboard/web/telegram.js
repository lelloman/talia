// Trusted administrative shell. No bot credential is saved in browser storage.
export async function startTelegram(account){
 const $=id=>document.getElementById(id),root=$('telegram-settings');
 let state,busy=false;
 const status=text=>$('telegram-status').textContent=text;
 function button(text,action){const b=document.createElement('button');b.textContent=text;b.onclick=()=>attempt(action);return b;}
 async function attempt(fn){if(busy)return;busy=true;root.setAttribute('aria-busy','true');try{await fn();await refresh();}catch(e){status(e.message);}finally{busy=false;root.removeAttribute('aria-busy');}}
 async function refresh(){
  root.hidden=!!window.taliaViewer;if(root.hidden)return;
  state=await account({op:'telegramStatus'});
  $('telegram-enabled').checked=state.enabled;
  status(state.error||(!state.bot?'No bot connected.':(state.enabled?'Connected to @':'Paused: @')+state.bot+(state.lastPoll?' · Last poll '+new Date(state.lastPoll).toLocaleString():'')));
  $('telegram-pairs').replaceChildren(...state.pairs.map(p=>{
   const row=document.createElement('div');row.style.overflowWrap='anywhere';
   if(!p.candidate){const text=document.createElement('p');text.textContent='Send /pair '+p.code+' in the destination chat (expires '+new Date(p.expires).toLocaleTimeString()+'). Then refresh.';row.append(text);return row;}
   const c=p.candidate,label=document.createElement('p');label.textContent=`${c.name} · ${c.kind} chat ${c.chat} · account ${c.user_name||'anonymous/channel'} (${c.user??'none'})`;row.append(label);
   row.append(button('Approve delivery only',()=>account({op:'telegramApprove',code:p.code,delivery:true,investigate:false})));
   return row;
  }));
  $('telegram-peers').replaceChildren(...state.peers.map(p=>{
   const row=document.createElement('div'),label=document.createElement('p');label.textContent=`${p.name} (${p.chat}) · Destination: telegram-${p.chat}`;row.append(label);
   for(const [field,title] of [['delivery','Report and alert delivery'],['investigate','Previously approved investigations (unavailable)']]){const l=document.createElement('label'),c=document.createElement('input');c.type='checkbox';c.checked=p[field];c.disabled=field==='investigate';c.onchange=()=>attempt(()=>account({op:'telegramPeer',peer:{...p,[field]:c.checked}}));l.append(c,document.createTextNode(title));row.append(l);}
   if(p.delivery)row.append(button('Send test message',()=>account({op:'telegramTest',chat:p.chat})));return row;
  }));
  $('telegram-users').replaceChildren(...state.users.map(u=>{const row=document.createElement('div');row.append(document.createTextNode(`${u.name} · ${u.id} `),button('Revoke investigation access',()=>account({op:'telegramRevokeUser',id:u.id})));return row;}));
  $('telegram-deliveries').textContent=state.deliveries.map(d=>`${d.status}: ${d.count}`).join(' · ');
  $('telegram-jobs').replaceChildren(...state.jobs.map(j=>{const p=document.createElement('p');p.textContent=`Investigation ${j.id}: ${j.status}${j.error?' · '+j.error:''}${j.session?' · Session '+j.session:''}`;return p;}));
 }
 $('telegram-connect').onclick=()=>attempt(async()=>{const token=$('telegram-token').value;try{await account({op:'telegramConnect',token,expected:state.version});}finally{$('telegram-token').value='';}});
 $('telegram-pair').onclick=()=>attempt(()=>account({op:'telegramPair'}));
 $('telegram-refresh').onclick=()=>attempt(async()=>{});
 $('telegram-save').onclick=()=>attempt(()=>account({op:'telegramSettings',expected:state.version,enabled:$('telegram-enabled').checked}));
 window.addEventListener('hashchange',()=>{if(location.hash==='#settings'&&!busy)attempt(async()=>{});else if(location.hash!=='#settings')$('telegram-token').value='';});
 await attempt(async()=>{});
}
