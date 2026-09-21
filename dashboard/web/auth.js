// Trusted shell: server decisions are authoritative; no provider tokens enter the VM.
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
 document.querySelector('#account-name').textContent=identity.name;
 document.querySelector('#sign-out').onclick=async()=>{const r=await fetch('/auth/logout',{method:'POST'});if(r.ok)location.replace('/');else document.querySelector('#shell-status').textContent='Sign out failed. Please retry.';};
 document.querySelector('#alert-access')?.closest('label')?.setAttribute('hidden','');document.querySelector('#alert-connect')?.setAttribute('hidden','');
 const account=async body=>{const r=await fetch('/account',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});if(r.status===401){location.replace('/');throw Error('Sign in required');}if(!r.ok)throw Error('Account connection failed');const v=await r.json();if(v.error)throw Error(v.error);return v;};
 const {startShell}=await import('./shell.js');await startShell(identity,account);
 setInterval(async()=>{try{const r=await fetch('/auth/session',{cache:'no-store'});if(r.status===401)location.replace('/');}catch{}},30000);
}
