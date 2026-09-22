// Framework-owned application chrome. The dashboard host owns one retained DOM island.
import {createApp,h,ref,computed,nextTick,watch} from 'vue';
import {LelloTheme,LelloScaffold,LelloAccount,LelloDialog,LelloButton,LelloConnectionStatus,LelloThemeSelector} from '@lelloman/lellodesign-vue';
import '@lelloman/lellodesign-vue/style.css';
const read=(key,fallback)=>{try{return localStorage.getItem(key)??fallback;}catch{return fallback;}};
const save=(key,value)=>{try{localStorage.setItem(key,value);}catch{}};
const paths={dashboard:'M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z',alerts:'M12 3a6 6 0 0 0-6 6v5l-2 3h16l-2-3V9a6 6 0 0 0-6-6ZM9 21h6',settings:'M4 6h16M4 12h16M4 18h16M8 3v6M16 9v6M10 15v6',sharing:'M8 12l8-6M8 12l8 6M8 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM22 4a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM22 20a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z',users:'M9 10a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM2 22v-4a7 7 0 0 1 14 0v4M17 3a4 4 0 0 1 0 8M20 22v-4a7 7 0 0 0-2-5'};
export async function mountChrome(identity,{development=false,signOut=()=>{}}={}){
 const preference=read('talia.appearance','system'),appearance=ref(['light','dark','system'].includes(preference)?preference:'system');
 const darkMedia=matchMedia('(prefers-color-scheme: dark)'),mobileMedia=matchMedia('(max-width:759px)');
 const systemDark=ref(darkMedia.matches),mobile=ref(mobileMedia.matches),collapsed=ref(read('talia.sidebarCollapsed','false')==='true');
 const active=ref('dashboard'),admin=ref(false),canAlert=ref(development),dialog=ref(false),status=ref('connecting');
 const theme=computed(()=>`blue-${appearance.value==='system'?(systemDark.value?'dark':'light'):appearance.value}`);
 const items=computed(()=>[{id:'dashboard',label:'Dashboard'},...(canAlert.value?[{id:'alerts',label:'Alerts'}]:[]),{id:'settings',label:'Settings'},...(admin.value?[{id:'sharing',label:'Sharing'},{id:'users',label:'Users'}]:[])].map(i=>({...i,href:'#'+i.id})));
 darkMedia.addEventListener('change',e=>systemDark.value=e.matches);mobileMedia.addEventListener('change',e=>mobile.value=e.matches);
 watch(appearance,v=>save('talia.appearance',v));watch(collapsed,v=>save('talia.sidebarCollapsed',String(v)));
 watch(theme,v=>{document.documentElement.style.colorScheme=v.endsWith('-dark')?'dark':'light';},{immediate:true});
 const island=document.querySelector('#host-content').content.cloneNode(true);let islandRoot;const scroll=new Map();
 function route(focus=false){const desired=location.hash.slice(1)||'dashboard';if(desired.startsWith('lello-main-'))return;const next=items.value.some(i=>i.id===desired)?desired:'dashboard';const main=document.querySelector('.lv-main');if(main)scroll.set(active.value,main.scrollTop);active.value=next;
  document.querySelectorAll('[data-shell-page]').forEach(p=>p.hidden=p.dataset.shellPage!==next);
  document.title=`${items.value.find(i=>i.id===next)?.label||'Dashboard'} · Talìa`;
  if(focus)requestAnimationFrame(()=>{document.querySelector(`[data-shell-page="${next}"] h1`)?.focus({preventScroll:true});if(main)main.scrollTop=scroll.get(next)||0;});
 }
 const app=createApp({setup(){return()=>h(LelloTheme,{theme:theme.value},{default:()=>[
  h(LelloScaffold,{productName:'Talìa',title:mobile.value?'Talìa':items.value.find(i=>i.id===active.value)?.label,items:items.value,active:active.value,mobileNavigation:'drawer',collapsed:collapsed.value,'onUpdate:collapsed':v=>collapsed.value=v,onNavigate:(item,e)=>{if(e.button!==0||e.ctrlKey||e.metaKey||e.shiftKey||e.altKey)return;e.preventDefault();if(location.hash!==item.href)location.hash=item.href;else route(true);}},{
   logo:()=>h('img',{src:'/assets/brand/brand.svg',alt:'',width:36,height:36}),
   'nav-icon':({item})=>h('svg',{viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':1.7,'stroke-linecap':'round','stroke-linejoin':'round'},[h('path',{d:paths[item.id]})]),
   account:({compact})=>h(LelloAccount,{name:identity.name,compact,label:development?'Local development':'Lello account',onActivate:()=>dialog.value=true}),
   'header-actions':()=>h('div',{class:'lv-header-controls'},[h(LelloConnectionStatus,{state:status.value,label:'Talìa connection'}),h(LelloThemeSelector,{modelValue:appearance.value,'onUpdate:modelValue':v=>appearance.value=v})]),
   default:()=>h('div',{class:'talia-host',ref:el=>{if(el&&!islandRoot){islandRoot=el;el.append(island);}}})
  }),
  h(LelloDialog,{title:development?'Development session':'Your Lello account',modelValue:dialog.value,'onUpdate:modelValue':v=>dialog.value=v},{default:()=>[h('p',{id:'account-name'},identity.name),h('p',{id:'account-role',class:'lv-muted'},development?'Development':admin.value?'Administrator':'Viewer')],actions:()=>[h(LelloButton,{variant:'neutral',onClick:()=>dialog.value=false},{default:()=> 'Close'}),...(!development?[h(LelloButton,{id:'sign-out',variant:'primary',onClick:signOut},{default:()=> 'Sign out'})]:[])]})
 ]});}});
 app.mount('#app');await nextTick();if(development)document.getElementById('agent-access').hidden=true;route();addEventListener('hashchange',()=>route(true));
 window.taliaShell={update({isAdmin=admin.value,alerts=canAlert.value,connected}={}){admin.value=isAdmin;canAlert.value=alerts;if(connected!==undefined)status.value=connected?'connected':'disconnected';route();},status(value){status.value=value==='connecting...'?'connecting':value==='disconnected'?'disconnected':'connected';}};
 addEventListener('talia-connection',e=>window.taliaShell.status(e.detail));
 return window.taliaShell;
}
