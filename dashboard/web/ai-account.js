// Only public authorization codes/identity reach this shell. Credentials stay on the server.
export async function startAiAccount(account){
 const $=id=>document.getElementById(id),root=$('ai-account-settings');
 let state,busy=false,timer,renderedVersion;
 const status=text=>$('ai-account-status').textContent=text;
 function render(){
  if(renderedVersion!==state.version){$('ai-account-origin').value=state.origin||'';$('ai-account-model').value=state.model||'';renderedVersion=state.version;}
  status(state.error||(state.phase==='connected'?'Connected · '+(state.identity.email||state.identity.name||state.identity.sub):state.phase==='unconfigured'?(state.legacy?.configured?'Using the installed API key · '+state.legacy.model:'No account connected.'):({pending:'Waiting for LelloAuth authorization…',polling:'Checking authorization…',validating:'Verifying the account…',confirm:'Check the account below and confirm.',refreshing:'Refreshing the saved session…',validating_refresh:'Verifying the refreshed session…',reconnect:'Reconnect the account to continue.',disconnected:'Disconnected.'}[state.phase]||state.phase)));
  const pending=state.pending;
  $('ai-account-pending').hidden=!pending;
  if(pending){$('ai-account-link').href=pending.url;$('ai-account-code').textContent=pending.code;}
  $('ai-account-identity').textContent=state.identity?.sub?`Account: ${state.identity.email||state.identity.name||''} (${state.identity.sub})`:'';
  $('ai-account-confirm').hidden=state.phase!=='confirm'||!state.owned;
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
  catch(e){status(e.message);clearTimeout(timer);}
  finally{busy=false;root.removeAttribute('aria-busy');}
 }
 $('ai-account-connect').onclick=()=>attempt(()=>account({op:'aiAccountBegin',expected:state.version,origin:$('ai-account-origin').value.trim(),model:$('ai-account-model').value.trim()}));
 $('ai-account-confirm').onclick=()=>attempt(()=>account({op:'aiAccountConfirm',expected:state.version}));
 $('ai-account-disconnect').onclick=()=>attempt(()=>account({op:'aiAccountDisconnect',expected:state.version}));
 $('ai-account-refresh').onclick=()=>state?.phase==='validating'?attempt(()=>account({op:'aiAccountPoll',expected:state.version})):refresh();
 window.addEventListener('hashchange',()=>{if(location.hash==='#settings')refresh();});
 await refresh();
}
