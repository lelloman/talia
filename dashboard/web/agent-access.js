// One-time key display: secrets never enter persistent browser storage or the dashboard VM.
export function startAgentAccess(account){
 const $=id=>document.getElementById(id),url=new URL('/mcp',location.origin).href;
 let key='',keyId='',expires=0,displayEpoch=0,refreshEpoch=0;
 $('agent-endpoint').value=url;
 const say=text=>$('agent-key-status').textContent=text;
 function forget(){displayEpoch++;key='';keyId='';expires=0;$('agent-key-value').value='';$('agent-key-created').hidden=true;}
 async function attempt(work){try{await work();}catch(e){say('Could not complete the request: '+e.message);}}
 async function refresh(){const epoch=++refreshEpoch,shownKey=keyId;const {keys}=await account({op:'agentKeys'});if(epoch!==refreshEpoch)return;$('agent-key-list').replaceChildren(...keys.map(k=>{const row=document.createElement('div');row.className='agent-key-row';const text=document.createElement('span');text.textContent=`Created ${new Date(k.created).toLocaleTimeString()} · expires ${new Date(k.expires).toLocaleTimeString()}`;const revoke=document.createElement('button');revoke.textContent='Revoke';revoke.onclick=()=>attempt(async()=>{revoke.disabled=true;try{await account({op:'agentKeyRevoke',id:k.id});if(keyId===k.id)forget();say('Agent key revoked.');await refresh();}finally{revoke.disabled=false;}});row.append(text,revoke);return row;}));if(!keys.length)$('agent-key-list').textContent='No active keys.';if(keyId===shownKey&&keyId&&!keys.some(k=>k.id===keyId))forget();}
 $('agent-key-create').onclick=()=>attempt(async()=>{const button=$('agent-key-create');button.disabled=true;forget();const epoch=displayEpoch;try{const result=await account({op:'agentKeyCreate'});if(epoch!==displayEpoch||location.hash!=='#settings'){await account({op:'agentKeyRevoke',id:result.id});return;}key=result.key;keyId=result.id;expires=result.expires;$('agent-key-value').value=key;$('agent-key-created').hidden=false;say(`Key expires at ${new Date(expires).toLocaleTimeString()}. Copy it now; it cannot be shown again.`);await refresh();}finally{button.disabled=false;}});
 $('agent-key-copy').onclick=()=>attempt(async()=>{await navigator.clipboard.writeText(key);say('Key copied.');});
 $('agent-config-copy').onclick=()=>attempt(async()=>{await navigator.clipboard.writeText(JSON.stringify({mcpServers:{talia:{type:'http',url,headers:{Authorization:`Bearer ${key}`}}}},null,2));say('MCP configuration copied.');});
 $('agent-endpoint-copy').onclick=()=>attempt(async()=>{await navigator.clipboard.writeText(url);say('MCP URL copied.');});
 $('agent-key-dismiss').onclick=forget;
 addEventListener('hashchange',()=>{if(location.hash==='#settings')attempt(refresh);else forget();});
 setInterval(()=>{if(expires&&Date.now()>=expires)forget();if(location.hash==='#settings'&&!document.hidden)attempt(refresh);},10000);
 if(location.hash==='#settings')attempt(refresh);
}
