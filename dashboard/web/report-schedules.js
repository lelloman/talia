import {createApp,h,ref} from 'vue';
import {LelloSection,LelloButton,LelloInput,LelloSelect,LelloCheckbox} from '@lelloman/lellodesign-vue';

export function scheduleSummary(value){
 const s=value.schedule;if(!s)return 'No schedule';
 const days=['Mon','Tue','Wed','Thu','Fri','Sat','Sun'];
 const timing=s.kind==='daily'?`${s.weekdays?.length?s.weekdays.map(d=>days[d-1]).join(', '):'Every day'} at ${s.time} · ${s.zone}`:`Every ${s.every_ms/1000} seconds`;
 return value.enabled?timing:`Paused · ${timing}`;
}
export function scheduleDraft(draft){
 if(draft.kind==='none')return null;
 if(draft.kind==='interval'){
  const seconds=Number(draft.seconds);
  if(!Number.isSafeInteger(seconds*1000)||seconds<60||seconds>31536000)throw Error('Interval must be from 60 seconds to 365 days, with at most millisecond precision.');
  return {kind:'interval',every_ms:seconds*1000};
 }
 if(!/^([01]\d|2[0-3]):[0-5]\d$/.test(draft.time))throw Error('Use a time in HH:mm format.');
 const zone=draft.zone.trim();
 try{new Intl.DateTimeFormat('en',{timeZone:zone});}catch{throw Error('Choose an IANA timezone, for example Europe/Rome.');}
 return {kind:'daily',time:draft.time,zone,weekdays:[...draft.weekdays].sort((a,b)=>a-b)};
}
export function mountReportSchedules(account,subject){
 const rows=ref([]),current=ref(null),draft=ref(null),busy=ref(false),message=ref(''),pending=ref(null);
 const storageKey='talia.reportSchedule.'+encodeURIComponent(subject);
 try{pending.value=JSON.parse(localStorage.getItem(storageKey)||'null');}catch{}
 const rpc=(operation,args={})=>account({op:'nativeReports',operation,args});
 async function attempt(fn){if(busy.value)return;busy.value=true;message.value='';try{await fn();}catch(e){message.value=e.message;}finally{busy.value=false;}}
 async function refresh(){
  rows.value=(await rpc('list')).definitions;
  if(current.value)current.value=await rpc('schedule_get',{id:current.value.id});
 }
 function edit(){message.value="";const v=current.value,s=v.schedule;draft.value={enabled:v.enabled,version:v.version,kind:s?.kind||'none',time:s?.time||'09:00',zone:s?.zone||'Europe/Rome',seconds:String((s?.every_ms||3600000)/1000),weekdays:s?.weekdays||[]};}
 async function send(){
  const args=pending.value;
  try{
   const result=await rpc('schedule_save',args);
   localStorage.removeItem(storageKey);pending.value=null;draft.value=null;
   if(current.value?.id===result.id)current.value=result;
   await refresh();message.value='Schedule saved.';
  }catch(e){
   // An explicit server rejection did not commit; a transport failure is ambiguous.
   if(e.serverRejected){
    localStorage.removeItem(storageKey);pending.value=null;
    message.value=e.message+' Cancel editing, then refresh to review current settings.';
   }else message.value='Save not confirmed. Check the saved request before editing again. '+e.message;
  }
 }
 function save(){return attempt(async()=>{
  const d=draft.value,schedule=scheduleDraft(d);
  const args={id:current.value.id,enabled:d.enabled&&schedule!==null,schedule,expected:d.version,requestId:crypto.randomUUID()};
  // Persist before dispatch so a reload can safely resolve an ambiguous response.
  localStorage.setItem(storageKey,JSON.stringify(args));pending.value=args;await send();
 });}
 const button=(text,onClick,disabled=false,variant='neutral')=>h(LelloButton,{onClick,disabled:busy.value||disabled,variant},{default:()=>text});
 const input=(label,key,type='text')=>h(LelloInput,{label,type,modelValue:draft.value[key],disabled:busy.value||!!pending.value,'onUpdate:modelValue':v=>draft.value[key]=v});
 createApp({setup:()=>()=>h(LelloSection,{title:current.value?current.value.id:'Report schedules'},{default:()=>[
  h('p',{role:'status','aria-live':'polite'},message.value),
  pending.value?h('div',[h('p',`A schedule save for ${pending.value.id} needs confirmation.`),button('Check schedule save',()=>attempt(send))]):null,
  h('div',{class:'lv-row'},[button('Refresh',()=>attempt(refresh),!!draft.value),current.value?button('All reports',()=>{current.value=null;draft.value=null;},!!draft.value):null]),
  current.value?[
   h('p',scheduleSummary(current.value)),
   h('p',current.value.next_due?`Next run: ${new Intl.DateTimeFormat(undefined,{dateStyle:'full',timeStyle:'long',timeZone:current.value.schedule?.zone||'UTC'}).format(new Date(current.value.next_due))}`:'No scheduled run'),
   !draft.value?button('Edit schedule',edit,!!pending.value):h('div',{class:'lv-stack'},[
    h(LelloCheckbox,{label:'Schedule enabled',modelValue:draft.value.enabled,disabled:busy.value||!!pending.value||draft.value.kind==='none','onUpdate:modelValue':v=>draft.value.enabled=v}),
    h(LelloSelect,{label:'Repeat',options:[{value:'none',label:'None'},{value:'daily',label:'Daily'},{value:'interval',label:'Interval'}],modelValue:draft.value.kind,disabled:busy.value||!!pending.value,'onUpdate:modelValue':v=>{draft.value.kind=v;if(v==='none')draft.value.enabled=false;}}),
    ...(draft.value.kind==='daily'?[input('Time','time','time'),input('Timezone (IANA)','zone'),h('p','Weekdays · none selected means every day'),...['Monday','Tuesday','Wednesday','Thursday','Friday','Saturday','Sunday'].map((day,i)=>h(LelloCheckbox,{label:day,modelValue:draft.value.weekdays.includes(i+1),disabled:busy.value||!!pending.value,'onUpdate:modelValue':checked=>draft.value.weekdays=checked?[...draft.value.weekdays,i+1]:draft.value.weekdays.filter(d=>d!==i+1)}))]:draft.value.kind==='interval'?[input('Interval in seconds','seconds','number'),h('p','Elapsed interval; first run is one interval after enabling or changing it.')]:[]),
    h('p',current.value.destinations.length?`Scheduled runs send results to: ${current.value.destinations.join(', ')}`:'No delivery destinations configured. Enabling a schedule requires one.'),
    h('div',{class:'lv-row'},[button('Save schedule',save,!!pending.value,'primary'),button('Cancel',()=>draft.value=null,!!pending.value)]),
    h('p','Pausing affects future runs. Runs already started continue.')
   ])
  ]:rows.value.length?rows.value.map(row=>h('div',{class:'lv-row',key:row.id},[button(row.id,()=>attempt(async()=>{current.value=await rpc('schedule_get',{id:row.id});})),h('span',scheduleSummary(row))])):h('p','No reports configured.')
 ]})}).mount('#report-schedules');
 window.addEventListener('hashchange',()=>{if(location.hash==='#reports'&&!draft.value)attempt(refresh);});
 if(location.hash==='#reports')attempt(refresh);
}
