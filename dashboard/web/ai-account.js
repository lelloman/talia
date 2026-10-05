// Only public authorization codes/identity reach this shell. Credentials stay on the server.
export async function startAiAccount(account){
 const $=id=>document.getElementById(id),root=$('ai-account-settings');
 let state,busy=false,timer,renderedVersion;
 const status=text=>$('ai-account-status').textContent=text;
 function render(){
  if(renderedVersion!==state.version){$('ai-account-origin').value=state.origin||'';$('ai-account-model').value=state.model||'';renderedVersion=state.version;}
  status(state.error||(state.phase==='connected'?'Connected · '+(state.identity.email||state.identity.name||state.identity.sub):state.phase==='unconfigured'?(state.legacy?.configured?'Using the installed API key · '+state.legacy.model:'No account connected.'):({pending:'Waiting for LelloAuth authorization…',polling:'Checking authorization…',validating:'Verifying the account…',confirm:state.owned?'Authorized. Confirm the account below to start using it.':'Waiting for the administrator who started this connection to confirm it.',refreshing:'Refreshing the saved session…',validating_refresh:'Verifying the refreshed session…',reconnect:'Reconnect the account to continue.',disconnected:'Disconnected.'}[state.phase]||state.phase)));
  const pending=state.pending;
  $('ai-account-pending').hidden=!pending;
  if(pending){$('ai-account-link').href=pending.url;$('ai-account-code').textContent=pending.code;}
  const confirming=state.phase==='confirm'&&state.owned;
  $('ai-account-confirm-panel').hidden=!confirming;
  $('ai-account-identity').textContent=state.identity?.sub?`Account: ${state.identity.email||state.identity.name||''} (${state.identity.sub})`:'';
  $('ai-account-summary').textContent=`Model or class: ${state.model||'—'} · Server: ${state.origin||'—'}`;
  root.querySelector('.ai-account-status').dataset.tone=state.error?'error':confirming?'warning':state.phase==='connected'?'success':'';
  $('ai-account-connect').textContent=state.phase==='connected'?'Reconnect or change model':'Connect LelloAuth account';
  $('ai-account-disconnect').disabled=['unconfigured','disconnected'].includes(state.phase)&&!state.legacy?.configured;
  clearTimeout(timer);
  if(state.owned&&['pending','validating'].includes(state.phase))timer=setTimeout(()=>{if(!document.hidden&&location.hash==='#settings')attempt(()=>account({op:'aiAccountPoll',expected:state.version}));else scheduleRefresh();},Math.max(5,state.pending?.interval||5)*1000);
 }
 function scheduleRefresh(){clearTimeout(timer);timer=setTimeout(()=>refresh(),5000);}
 async function refresh(){return attempt(()=>account({op:'aiAccountStatus'}));}
 async function attempt(fn){
  root.hidden=!!window.taliaViewer;if(root.hidden){clearTimeout(timer);return;}
  if(busy)return;busy=true;root.setAttribute('aria-busy','true');
  try{state=await fn();render();window.dispatchEvent(new Event('talia-ai-account-changed'));}
  catch(e){
   clearTimeout(timer);
   // A page loaded before verification finished holds an old version: reload and ask again.
   if(/changed/i.test(e.message)){busy=false;root.removeAttribute('aria-busy');await refresh();status('The account state changed while you were on this page. Check it again below.');return;}
   status(e.message);
  }
  finally{busy=false;root.removeAttribute('aria-busy');}
 }
 $('ai-account-connect').onclick=()=>attempt(()=>account({op:'aiAccountBegin',expected:state.version,origin:$('ai-account-origin').value.trim(),model:$('ai-account-model').value.trim()}));
 $('ai-account-confirm').onclick=()=>attempt(()=>account({op:'aiAccountConfirm',expected:state.version}));
 $('ai-account-disconnect').onclick=()=>attempt(()=>account({op:'aiAccountDisconnect',expected:state.version}));
 $('ai-account-cancel').onclick=()=>attempt(()=>account({op:'aiAccountDisconnect',expected:state.version}));
 $('ai-account-refresh').onclick=()=>state?.phase==='validating'?attempt(()=>account({op:'aiAccountPoll',expected:state.version})):refresh();
 window.addEventListener('hashchange',()=>{if(location.hash==='#settings')refresh();});
 await refresh();
}
