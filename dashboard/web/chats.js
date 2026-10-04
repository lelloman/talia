// Administrator chat with Talìa's read-only investigation tools (server-owned sessions).
import {createApp,h,ref,computed,nextTick,watch} from 'vue';
import {LelloButton,LelloTextarea,LelloInput,LelloAlert,LelloBadge,LelloConfirmDialog} from '@lelloman/lellodesign-vue';

const active=r=>r.status==='queued'||r.status==='running';
const store={
 get(key){try{return JSON.parse(localStorage.getItem(key)||'null');}catch{return null;}},
 set(key,value){try{value===null?localStorage.removeItem(key):localStorage.setItem(key,JSON.stringify(value));}catch{}},
};
function when(ms){
 const d=new Date(ms),now=new Date();
 return d.toDateString()===now.toDateString()?d.toLocaleTimeString([], {hour:'2-digit',minute:'2-digit'}):d.toLocaleDateString([], {month:'short',day:'numeric'});
}

export function mountChats(account,subject){
 const prefix='talia.chat.'+encodeURIComponent(subject)+'.';
 const sessions=ref([]),current=ref(null),requests=ref([]),loaded=ref(false);
 const draft=ref(''),title=ref(null),confirmDelete=ref(false),busy=ref(false);
 const error=ref(''),forbidden=ref(false),showList=ref(true);
 // A send whose response was lost keeps its requestId so a retry cannot duplicate it.
 const unsent=ref(null);
 const rpc=(op,args={})=>account({op:'chat'+op[0].toUpperCase()+op.slice(1),...args});
 const draftKey=()=>prefix+'draft.'+(current.value||'new');
 const pendingKey=()=>prefix+'unsent.'+(current.value||'new');
 function loadDraft(){draft.value=store.get(draftKey())||'';unsent.value=store.get(pendingKey());if(unsent.value)draft.value=unsent.value.text;}
 watch(draft,v=>store.set(draftKey(),v||null));
 function fail(e){
  if(e.message==='forbidden'){forbidden.value=true;return;}
  error.value=e.message==='limit_exceeded'?'Too many requests are running. Wait for an answer, or stop one, then try again.'
   :e.message==='not_found'?'This chat is no longer available.':e.message;
 }
 async function refreshList(){
  try{sessions.value=(await rpc('list')).sessions;forbidden.value=false;loaded.value=true;}catch(e){fail(e);}
 }
 async function refreshSession(){
  const id=current.value;if(!id)return;
  try{const v=await rpc('get',{session:id});if(current.value!==id)return;requests.value=v.requests;}
  catch(e){if(e.message==='not_found'){current.value=null;requests.value=[];await refreshList();}fail(e);}
 }
 async function open(id){
  current.value=id;requests.value=[];title.value=null;error.value='';showList.value=false;loadDraft();
  await refreshSession();scrollToEnd();
 }
 function startNew(){current.value=null;requests.value=[];title.value=null;error.value='';showList.value=false;loadDraft();
  nextTick(()=>document.querySelector('.talia-chat-composer textarea')?.focus());}
 function scrollToEnd(){nextTick(()=>{const log=document.querySelector('.talia-chat-log');if(log)log.scrollTop=log.scrollHeight;});}
 async function send(){
  const text=draft.value.trim();if(!text||busy.value)return;
  const pending=unsent.value&&unsent.value.text===text?unsent.value:{requestId:crypto.randomUUID(),text};
  // Persist before dispatch, so a reload can safely retry the same request.
  unsent.value=pending;store.set(pendingKey(),pending);busy.value=true;error.value='';
  try{
   const result=current.value?await rpc('send',{session:current.value,...pending}):await rpc('create',pending);
   store.set(pendingKey(),null);unsent.value=null;draft.value='';store.set(draftKey(),null);
   if(!current.value){current.value=result.session;}
   await Promise.all([refreshSession(),refreshList()]);scrollToEnd();
  }catch(e){
   // An explicit server rejection did not admit the message; a lost response is ambiguous.
   if(e.serverRejected){store.set(pendingKey(),null);unsent.value=null;}
   fail(e);if(!e.serverRejected)error.value='Message not confirmed. Retry sends it once; it will not be duplicated.';
  }finally{busy.value=false;}
 }
 const act=fn=>async()=>{if(busy.value)return;busy.value=true;error.value='';try{await fn();}catch(e){fail(e);}finally{busy.value=false;}};
 const stop=act(async()=>{await rpc('stop',{session:current.value});await refreshSession();await refreshList();});
 const rename=act(async()=>{const name=title.value.trim();if(!name)return;await rpc('rename',{session:current.value,title:name});title.value=null;await refreshList();});
 const remove=act(async()=>{await rpc('delete',{session:current.value});store.set(draftKey(),null);current.value=null;requests.value=[];showList.value=true;await refreshList();});
 const again=r=>{draft.value=r.text;unsent.value=null;nextTick(()=>document.querySelector('.talia-chat-composer textarea')?.focus());};
 const session=computed(()=>sessions.value.find(s=>s.id===current.value));
 const running=computed(()=>requests.value.some(active));
 // Poll quickly only while an answer is being worked on.
 let timer=null;
 function schedule(){clearTimeout(timer);timer=setTimeout(async()=>{if(location.hash==='#chats'&&!document.hidden){await refreshSession();if(!running.value)await refreshList();}schedule();},running.value?1500:10000);}
 watch(running,(now,before)=>{schedule();if(before&&!now){refreshList();scrollToEnd();}});

 const message=r=>h('li',{class:'talia-chat-turn',key:r.id},[
  h('div',{class:'talia-chat-question'},[h('p',r.text),r.report?h('p',{class:'talia-chat-meta'},'Report run '+r.report):null]),
  r.status==='done'?h('div',{class:'talia-chat-answer'},h('p',r.answer)):
  active(r)?h('div',{class:'talia-chat-answer talia-chat-working',role:'status','aria-live':'polite'},[
   h('p',{class:'talia-chat-working-title'},[h('span',{class:'talia-chat-spinner','aria-hidden':'true'}),r.status==='queued'?'Waiting to start…':'Investigating…']),
   r.steps?.length?h('ul',{class:'talia-chat-steps'},r.steps.map((s,i)=>h('li',{key:i,class:s.done?'done':'current'},s.label))):null]):
  h('div',{class:'talia-chat-answer'},[
   h(LelloAlert,{tone:r.status==='stopped'?'info':'error'},{default:()=>r.status==='stopped'?'Stopped before an answer was ready.':r.status==='cancelled'?'Cancelled.':(r.error||'The investigation failed.')}),
   h(LelloButton,{variant:'ghost',onClick:()=>again(r)},{default:()=>'Try again'})])
 ]);
 const list=()=>h('nav',{class:'talia-chat-list','aria-label':'Chats'},[
  h(LelloButton,{variant:'primary',class:'talia-chat-new',onClick:startNew},{default:()=>'New chat'}),
  sessions.value.length?h('ul',sessions.value.map(s=>h('li',{key:s.id},h('button',{class:['talia-chat-item',{selected:s.id===current.value}],'aria-current':s.id===current.value?'true':null,onClick:()=>open(s.id)},[
   h('span',{class:'talia-chat-item-title'},s.title),
   h('span',{class:'talia-chat-item-meta'},[s.running?h(LelloBadge,{tone:'info'},{default:()=>'Running'}):null,h('span',when(s.updated))])])))):
  loaded.value?h('p',{class:'talia-chat-empty-list'},'No chats yet.'):null]);
 const conversation=()=>h('section',{class:'talia-chat-conversation','aria-label':session.value?.title||'New chat'},[
  h('header',{class:'talia-chat-header'},[
   h(LelloButton,{variant:'ghost',class:'talia-chat-back',onClick:()=>showList.value=true},{default:()=>'‹ Chats'}),
   title.value!==null?h('form',{class:'talia-chat-rename',onSubmit:e=>{e.preventDefault();rename();}},[
     h(LelloInput,{label:'Chat title',modelValue:title.value,'onUpdate:modelValue':v=>title.value=v,maxlength:120}),
     h(LelloButton,{type:'submit',variant:'primary',disabled:busy.value},{default:()=>'Save'}),
     h(LelloButton,{variant:'ghost',onClick:()=>title.value=null},{default:()=>'Cancel'})]):
    h('h2',{class:'talia-chat-title'},session.value?.title||'New chat'),
   current.value&&title.value===null?h('div',{class:'talia-chat-actions'},[
    running.value?h(LelloButton,{variant:'neutral',onClick:stop,disabled:busy.value},{default:()=>'Stop'}):null,
    h(LelloButton,{variant:'ghost',onClick:()=>title.value=session.value?.title||''},{default:()=>'Rename'}),
    h(LelloButton,{variant:'ghost',onClick:()=>confirmDelete.value=true},{default:()=>'Delete'})]):null]),
  h('ol',{class:'talia-chat-log'},requests.value.length?requests.value.map(message):[h('li',{class:'talia-chat-intro'},[
   h('h3','Ask Talìa about your homelab'),
   h('p','Talìa can read current monitoring values, history, alerts and report runs, and query approved diagnostic sources. It never changes anything.')])]),
  error.value?h(LelloAlert,{tone:'error',class:'talia-chat-error'},{default:()=>error.value}):null,
  h('form',{class:'talia-chat-composer',onSubmit:e=>{e.preventDefault();send();}},[
   h(LelloTextarea,{label:'Message',modelValue:draft.value,'onUpdate:modelValue':v=>draft.value=v,rows:1,maxlength:8000,placeholder:'Ask a question…',
    onKeydown:e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.isComposing){e.preventDefault();send();}}}),
   h(LelloButton,{type:'submit',variant:'primary',loading:busy.value,disabled:!draft.value.trim()},{default:()=>unsent.value?'Retry':'Send'})]),
  h(LelloConfirmDialog,{modelValue:confirmDelete.value,'onUpdate:modelValue':v=>confirmDelete.value=v,title:'Delete this chat?',
   message:'The conversation will be removed from your chats. Running work stops.',confirmLabel:'Delete chat',danger:true,onConfirm:remove})]);
 createApp({render:()=>forbidden.value?h(LelloAlert,{tone:'warning',title:'Chat unavailable'},{default:()=>'Chat requires administrator access.'}):
  h('div',{class:['talia-chat',{'show-list':showList.value}]},[list(),conversation()])}).mount('#chats');
 const enter=()=>{if(location.hash==='#chats'){refreshList();refreshSession();}};
 addEventListener('hashchange',enter);document.addEventListener('visibilitychange',()=>{if(!document.hidden)enter();});
 loadDraft();enter();schedule();
}
