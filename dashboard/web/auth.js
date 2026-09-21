// Trusted host authentication. OIDC/provider/session credentials never enter the VM.
const response=await fetch('/auth/session',{cache:'no-store'});
if(response.status===404){await import('./app.js');}
else if(!response.ok){location.replace('/');}
else {
 const identity=await response.json();window.taliaOidc=true;
 const previous=localStorage.getItem('talia.oidcSubject');
 if(previous!==identity.subject){
  for(const key of Object.keys(localStorage))if(key.startsWith('talia.registration.')||key.startsWith('talia.baseline.'))localStorage.removeItem(key);
  sessionStorage.removeItem('talia.slot.v1');sessionStorage.removeItem('talia.alertCredential');
  localStorage.setItem('talia.oidcSubject',identity.subject);
 }
 const header=document.querySelector('header'),name=document.createElement('span'),logout=document.createElement('button');
 name.textContent=identity.name;logout.textContent='Sign out';header.append(name,logout);
 logout.onclick=async()=>{const r=await fetch('/auth/logout',{method:'POST'});if(r.ok)location.replace('/');else logout.textContent='Sign out failed — retry';};
 document.querySelector('#alert-access')?.closest('label')?.setAttribute('hidden','');document.querySelector('#alert-connect')?.setAttribute('hidden','');
 // A lost or revoked server session returns to the sign-in screen.
 setInterval(async()=>{try{const r=await fetch('/auth/session',{cache:'no-store'});if(r.status===401)location.replace('/');}catch{}},30000);
 await import('./app.js');
}
