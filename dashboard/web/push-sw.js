// No fetch interception or offline app cache: this worker only handles Web Push.
self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));
self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));
self.addEventListener('push',event=>event.waitUntil((async()=>{
 let payload;try{payload=event.data?.json();}catch{return;}
 if(payload?.version!==1||typeof payload.device!=='string'||typeof payload.key!=='string'||!Number.isSafeInteger(payload.occurrence)||!Number.isSafeInteger(payload.revision)||!Number.isSafeInteger(payload.expires)||payload.expires<=Date.now())return;
 try{
  const response=await fetch('/account',{method:'POST',credentials:'same-origin',cache:'no-store',signal:AbortSignal.timeout(5000),headers:{'Content-Type':'application/json'},body:JSON.stringify({op:'browserPushAlert',id:payload.device,key:payload.key,occurrence:payload.occurrence,revision:payload.revision})});
  if(!response.ok)return;const body=await response.json();if(body.error||!body.alert)return;
  if(payload.expires<=Date.now())return;
  const a=body.alert;
  await self.registration.showNotification('Talìa · '+(a.active?a.severity:'Recovered'),{body:a.message,tag:'talia:'+a.key,icon:'/assets/brand/favicon.svg',data:{url:'/#alerts',expires:payload.expires},renotify:true});
 }catch{
  if(payload.expires<=Date.now())return;
  // Offline wakeup has no authenticated details to show. A generic notice also
  // satisfies userVisibleOnly without exposing a previous account's alert text.
  await self.registration.showNotification('Talìa',{body:'An alert update is available. Open Talìa to check.',tag:'talia:connection',data:{url:'/#alerts',expires:payload.expires}});
 }
})()));
self.addEventListener('notificationclick',event=>{
 event.notification.close();event.waitUntil((async()=>{
  const url=new URL('/#alerts',self.location.origin).href;
  const windows=await self.clients.matchAll({type:'window',includeUncontrolled:true});
  const existing=windows.find(w=>new URL(w.url).origin===self.location.origin);
  if(existing){await existing.navigate(url);await existing.focus();}else await self.clients.openWindow(url);
 })());
});

self.addEventListener('pushsubscriptionchange',event=>event.waitUntil((async()=>{
 for(const client of await self.clients.matchAll({type:'window',includeUncontrolled:true}))client.postMessage({type:'talia-push-subscription-changed'});
})()));
