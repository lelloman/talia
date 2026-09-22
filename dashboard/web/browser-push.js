// Trusted browser host only; subscriptions never enter a dashboard VM.
export async function startBrowserPush(identity,account){
 const status=document.querySelector('#browser-push-status'),enable=document.querySelector('#browser-push-enable'),disable=document.querySelector('#browser-push-disable'),destination=document.querySelector('#browser-push-destination');
 let registration,config,busy=false;
 const storageKey='talia.browserPush';
 let local;try{local=JSON.parse(localStorage.getItem(storageKey));}catch{}
 const owned=()=>local?.owner===identity.subject;
 const supported=()=>isSecureContext&&'serviceWorker' in navigator&&'PushManager' in window&&'Notification' in window;
 async function fingerprint(sub){return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify(sub.toJSON())))),v=>v.toString(16).padStart(2,'0')).join('');}
 function save(){localStorage.setItem(storageKey,JSON.stringify(local));}
 async function refresh(){
  if(busy)return;enable.disabled=true;disable.disabled=true;
  if(!supported()){status.textContent='Browser notifications are not supported here.';return;}
  if(window.taliaViewer){status.textContent='Browser alert notifications currently require an administrator account.';return;}
  try{
   config=await account({op:'browserPushConfig'});
   registration=await navigator.serviceWorker.register('/push-sw.js',{scope:'/'});
   await navigator.serviceWorker.ready;
   const sub=await registration.pushManager.getSubscription();
   const state=owned()?await account({op:'browserPushStatus',id:local.id}):{};
   const matchingKey=owned()&&local.applicationServerKey===config.applicationServerKey;
   const active=matchingKey&&!!sub&&state.device?.enabled&&Notification.permission==='granted';
   if(active){const digest=await fingerprint(sub);if(local.subscriptionDigest!==digest){await account({op:'browserPushRegister',id:local.id,subscription:sub.toJSON(),applicationServerKey:config.applicationServerKey});local.subscriptionDigest=digest;save();}}
   destination.textContent=active?'Destination: '+local.id:'';
   status.textContent=active?'Browser notifications enabled.':Notification.permission==='denied'?'Notifications are blocked. Allow them in your browser’s site settings.':'Browser notifications are disabled.';
   enable.disabled=active||Notification.permission==='denied';disable.disabled=!owned()||!state.device?.enabled;
   if(owned()&&state.device?.enabled&&(!sub||Notification.permission==='denied')){await account({op:'browserPushDisable',id:local.id});disable.disabled=true;}
  }catch(e){status.textContent=e.message;disable.disabled=!owned();}
 }
 async function stop(){
  if(!supported()||!owned())return;
  if(owned())await account({op:'browserPushDisable',id:local.id});
  const reg=registration||await navigator.serviceWorker.getRegistration('/');
  const sub=await reg?.pushManager.getSubscription();if(sub)await sub.unsubscribe();
  if(reg)for(const notification of await reg.getNotifications())notification.close();
 }
 enable.onclick=async()=>{
  if(busy)return;busy=true;enable.disabled=true;
  try{
   // Permission is requested directly from the user gesture, never during load.
   if(await Notification.requestPermission()!=='granted')throw Error('Notification permission was not granted.');
   config=await account({op:'browserPushConfig'});
   registration=await navigator.serviceWorker.register('/push-sw.js',{scope:'/'});await navigator.serviceWorker.ready;
   let sub=await registration.pushManager.getSubscription();
   // Explicit re-enrollment replaces even a locally cached subscription that
   // the push service may already have expired. Preserve the installation ID.
   if(sub){await sub.unsubscribe();sub=null;}
   const bytes=Uint8Array.from(atob(config.applicationServerKey.replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0));
   sub ||= await registration.pushManager.subscribe({userVisibleOnly:true,applicationServerKey:bytes});
   if(!owned())local={id:'browser-'+crypto.randomUUID(),owner:identity.subject};
   local.applicationServerKey=config.applicationServerKey;save();
   await account({op:'browserPushRegister',id:local.id,subscription:sub.toJSON(),applicationServerKey:config.applicationServerKey});
   local.subscriptionDigest=await fingerprint(sub);save();
  }catch(e){status.textContent='Could not enable notifications: '+e.message;busy=false;enable.disabled=false;return;}
  busy=false;await refresh();
 };
 disable.onclick=async()=>{if(busy)return;busy=true;disable.disabled=true;try{await stop();}catch(e){status.textContent='Could not disable notifications: '+e.message;busy=false;disable.disabled=false;return;}busy=false;await refresh();};
 window.taliaDisableBrowserPush=stop;
 window.addEventListener('hashchange',()=>{if(location.hash==='#settings')refresh();});
 if(supported())navigator.permissions?.query({name:'notifications'}).then(permission=>{permission.onchange=()=>refresh();}).catch(()=>{});
 window.addEventListener('focus',()=>{if(location.hash==='#settings')refresh();});
 navigator.serviceWorker?.addEventListener('message',event=>{if(event.data?.type==='talia-push-subscription-changed')refresh();});
 await refresh();
}
