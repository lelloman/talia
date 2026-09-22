// Trusted client presentation settings; no authored package or engine definition changes.
const idPattern=/^[A-Za-z][A-Za-z0-9_-]{0,63}$/;
export function normalizePlaylist(value){
 const seen=new Set(),entries=[];
 for(const entry of Array.isArray(value?.entries)?value.entries:[]){
  if(entries.length===32)break;
  if(typeof entry?.id!=='string'||!idPattern.test(entry.id)||seen.has(entry.id)||!Number.isInteger(entry.seconds)||entry.seconds<5||entry.seconds>3600)continue;
  seen.add(entry.id);entries.push({id:entry.id,seconds:entry.seconds});
 }
 return {version:1,auto:value?.auto===true,entries};
}
// One switch at a time; dwell begins after successful load, never catches up missed time.
export class DashboardPlaylist {
 constructor({current,select,blocked,notify,now=()=>performance.now()}){Object.assign(this,{current,select,blocked,notify,now,entries:[],active:false,playing:false,busy:false,deadline:null});}
 configure(entries){this.entries=entries;this.deadline=null;}
 pause(reason=''){this.playing=false;this.deadline=null;if(reason)this.notify(reason);}
 async step(direction=1,automatic=false){
  if(this.busy||!this.active||!this.entries.length)return;
  const reason=this.blocked(automatic);if(reason){this.pause(reason);return;}
  const index=this.entries.findIndex(e=>e.id===this.current());
  const next=this.entries[index<0?(direction<0?this.entries.length-1:0):(index+direction+this.entries.length)%this.entries.length];
  this.deadline=null;if(next.id===this.current())return;
  this.busy=true;
  try{await this.select(next.id);}catch(e){this.pause('Rotation paused: '+e.message);}finally{this.busy=false;this.deadline=null;}
 }
 tick(suspended=false){
  if(!this.active||!this.playing||this.busy||suspended){this.deadline=null;return;}
  const reason=this.blocked(true);if(reason){this.pause(reason);return;}
  if(this.entries.length<2){this.deadline=null;return;}
  const entry=this.entries.find(e=>e.id===this.current());
  if(!entry){void this.step(1,true);return;}
  if(this.deadline===null)this.deadline=this.now()+entry.seconds*1000;
  if(this.now()>=this.deadline)void this.step(1,true);
 }
}
export function startMonitoringMode(identity){
 const $=id=>document.getElementById(id),surface=$('monitoring-surface'),toolbar=$('monitoring-controls');
 const storageKey='talia.monitoring.v1.'+encodeURIComponent(identity.subject);
 let settings;try{settings=JSON.parse(localStorage.getItem(storageKey));}catch{}settings=normalizePlaylist(settings);
 let catalog=[],active=false,hideTimer,restoreFocus,fullscreenRequested=false,connection=$('connection').textContent,lockouts=[];
 const message=text=>{$('monitoring-status').textContent=text;};
 const current=()=>window.taliaDashboard?.id;
 const player=new DashboardPlaylist({current,select:id=>window.talia.selectDashboard(id,undefined,undefined,true),blocked:automatic=>{
  if(window.dashboardReport?.dirty||!$('dirty').hidden)return 'Rotation paused: temporary dashboard edits. Exit monitoring mode and reload to discard them.';
  if(automatic&&!$('diagnostic').hidden)return 'Rotation paused: the dashboard needs attention.';
  if(!window.talia||!current())return 'No dashboard is ready.';
  return '';
 },notify:message});
 function persist(){try{localStorage.setItem(storageKey,JSON.stringify(settings));$('playlist-status').textContent='Saved for this browser and account.';}catch{$('playlist-status').textContent='Browser storage unavailable; settings last only for this session.';}}
 function configure(){player.configure(settings.entries.filter(e=>catalog.includes(e.id)));updateControls();}
 function updateControls(){
  $('monitoring-current').textContent=current()||'Dashboard';
  $('monitoring-play').textContent=player.playing?'Pause rotation':'Play rotation';
  $('monitoring-play').setAttribute('aria-pressed',String(player.playing));
  $('monitoring-play').disabled=player.busy||player.entries.length<2;
  $('monitoring-previous').disabled=$('monitoring-next').disabled=player.busy||player.entries.length<2;
  $('monitoring-enter').disabled=!current()||!catalog.includes(current());
 }
 function renderSettings(){
  $('playlist-auto').checked=settings.auto;
  $('playlist-add-dashboard').replaceChildren(...catalog.filter(id=>!settings.entries.some(e=>e.id===id)).map(id=>new Option(id,id)));
  $('playlist-add').disabled=!$('playlist-add-dashboard').options.length||settings.entries.length>=32;
  $('playlist-entries').replaceChildren(...settings.entries.map((entry,index)=>{
   const row=document.createElement('li'),name=document.createElement('span'),duration=document.createElement('input'),label=document.createElement('label');
   name.textContent=entry.id+(catalog.includes(entry.id)?'':' (unavailable — skipped)');
   duration.type='number';duration.min='5';duration.max='3600';duration.step='1';duration.value=entry.seconds;duration.setAttribute('aria-label','Seconds for '+entry.id);
   duration.onchange=()=>{const n=Number(duration.value);if(!Number.isInteger(n)||n<5||n>3600){duration.value=entry.seconds;$('playlist-status').textContent='Duration must be a whole number from 5 to 3600 seconds.';return;}entry.seconds=n;persist();configure();};
   label.append('Seconds ',duration);row.append(name,label);
   for(const [text,delta] of [['Move up',-1],['Move down',1],['Remove',0]]){
    const button=document.createElement('button');button.textContent=text;button.setAttribute('aria-label',text+' '+entry.id);button.disabled=delta===-1&&index===0||delta===1&&index===settings.entries.length-1;
    button.onclick=()=>{if(delta)[settings.entries[index],settings.entries[index+delta]]=[settings.entries[index+delta],settings.entries[index]];else settings.entries.splice(index,1);persist();configure();renderSettings();$('playlist-add-dashboard').focus();};row.append(button);
   }return row;
  }));
 }
 function reveal(){
  if(!active)return;surface.classList.remove('monitoring-controls-hidden');clearTimeout(hideTimer);
  hideTimer=setTimeout(()=>{surface.classList.add('monitoring-controls-hidden');},3000);
 }
 function isolate(){
  // In-page fallback needs the same keyboard isolation as native fullscreen.
  for(let node=surface;node.parentElement;node=node.parentElement){for(const sibling of node.parentElement.children)if(sibling!==node){lockouts.push([sibling,sibling.inert]);sibling.inert=true;}}
 }
 function leave(){
  if(!active)return;active=false;player.active=false;player.pause();clearTimeout(hideTimer);
  surface.classList.remove('monitoring-active','monitoring-controls-hidden');
  for(const [node,inert] of lockouts)node.inert=inert;lockouts=[];
  if(document.fullscreenElement===surface)document.exitFullscreen().catch(()=>{});
  fullscreenRequested=false;restoreFocus?.focus({preventScroll:true});updateControls();
 }
 $('monitoring-enter').onclick=async()=>{
  if(active)return;active=true;restoreFocus=document.activeElement;location.hash='#dashboard';surface.hidden=false;
  surface.classList.add('monitoring-active');isolate();surface.focus({preventScroll:true});
  player.active=true;player.playing=settings.auto;player.deadline=null;message('');reveal();updateControls();
  try{if(!surface.requestFullscreen)throw Error('unsupported');fullscreenRequested=true;await surface.requestFullscreen();if(!active&&document.fullscreenElement===surface)await document.exitFullscreen();}
  catch{fullscreenRequested=false;if(active)message('Browser fullscreen is unavailable. Monitoring mode is active in this window.');}
  if(active&&player.entries.length&&!player.entries.some(e=>e.id===current()))await player.step(1);
  updateControls();
 };
 $('monitoring-exit').onclick=leave;
 $('monitoring-previous').onclick=()=>{message('');void player.step(-1).then(updateControls);reveal();};
 $('monitoring-next').onclick=()=>{message('');void player.step(1).then(updateControls);reveal();};
 $('monitoring-play').onclick=()=>{player.playing=!player.playing;player.deadline=null;message('');updateControls();reveal();};
 $('playlist-add').onclick=()=>{const id=$('playlist-add-dashboard').value;if(!id||settings.entries.length>=32||settings.entries.some(e=>e.id===id))return;settings.entries.push({id,seconds:30});persist();configure();renderSettings();};
 $('playlist-auto').onchange=()=>{settings.auto=$('playlist-auto').checked;persist();};
 surface.addEventListener('pointermove',reveal);surface.addEventListener('pointerdown',reveal);toolbar.addEventListener('focusin',reveal);
 document.addEventListener('keydown',event=>{if(!active)return;reveal();if(event.key==='Escape'){event.preventDefault();leave();}});
 document.addEventListener('fullscreenchange',()=>{if(fullscreenRequested&&document.fullscreenElement!==surface)leave();});
 window.addEventListener('hashchange',()=>{if(active&&location.hash!=='#dashboard')leave();});
 window.addEventListener('talia-dashboard-loaded',()=>{player.deadline=null;updateControls();});
 window.addEventListener('talia-connection',event=>{connection=event.detail;$('monitoring-connection').textContent=connection;});
 $('monitoring-connection').textContent=connection;
 document.addEventListener('visibilitychange',()=>{player.deadline=null;});
 setInterval(()=>{if(!active)return;player.tick(document.hidden||connection==='connecting...'||connection==='disconnected');updateControls();},250);
 renderSettings();updateControls();
 return {setCatalog(dashboards){const next= dashboards.map(d=>d.id);if(JSON.stringify(next)===JSON.stringify(catalog))return;catalog=next;configure();renderSettings();if(active&&current()&&!catalog.includes(current())){player.pause('Rotation paused: access to this dashboard was removed.');}},leave};
}
