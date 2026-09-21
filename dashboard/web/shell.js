const $=id=>document.getElementById(id);
export async function startShell(identity,account){
 let catalog,users=[],started=false,busy=false;
 const message=text=>$('shell-status').textContent=text;
 async function attempt(work){try{await work();}catch(e){message('Could not complete the request: '+e.message);}}
 function current(){return catalog.dashboards.find(d=>d.id===$('dashboard-picker').value);}
 function sharing(){const d=current();$('share-public').checked=d?.access?.public===true;const allowed=d?.access?.viewers||[];for(const o of $('share-users').options)o.selected=allowed.includes(o.value);$('make-default').disabled=!d||catalog.defaultDashboard===d.id;$('make-default').textContent=catalog.defaultDashboard===d?.id?'Your default':'Use as my default';}
 async function refresh(){
  if(busy)return;busy=true;try{
   const next=await account({op:'catalog'});
   if(catalog&&catalog.admin!==next.admin){location.reload();return;}catalog=next;window.taliaViewer=!catalog.admin;
   $('account-role').textContent=catalog.admin?'Administrator':'Viewer';$('sharing').hidden=!catalog.admin;$('user-admin').hidden=!catalog.admin;$('sidebar').closest('label').hidden=!catalog.admin;
   const previous=window.taliaDashboard?.id||$('dashboard-picker').value;
   $('dashboard-picker').replaceChildren(...catalog.dashboards.map(d=>new Option(d.id,d.id)));
   if(catalog.dashboards.some(d=>d.id===previous))$('dashboard-picker').value=previous;
   $('empty-dashboard').hidden=catalog.dashboards.length>0;$('dashboard').hidden=!catalog.dashboards.length;$('reload').disabled=!catalog.dashboards.length;$('open-dashboard').disabled=!catalog.dashboards.length;
   if(catalog.admin){users=await account({op:'users'});$('share-users').replaceChildren(...users.map(u=>new Option(u.name,u.subject)));$('user-list').replaceChildren(...users.map(u=>{const row=document.createElement('label');const name=document.createElement('span');name.textContent=u.name;const role=document.createElement('select');role.add(new Option('Viewer','viewer'));role.add(new Option('Admin','admin'));role.value=u.admin?'admin':'viewer';role.disabled=u.subject===identity.subject;role.onchange=()=>attempt(async()=>{await account({op:'role',subject:u.subject,admin:role.value==='admin'});message('User access updated.');await refresh();});row.append(name,role);return row;}));}
   sharing();
   if(!started)$('connection').hidden=true;
   if(!started&&catalog.dashboards.length){started=true;await import('./app.js');}
   if(window.taliaDashboard&&!catalog.dashboards.some(d=>d.id===window.taliaDashboard.id)){window.talia?.revoke();message('Access to this dashboard has been removed. Choose an available dashboard.');}
  }finally{busy=false;}
 }
 $('dashboard-picker').onchange=()=>attempt(async()=>{await window.talia.selectDashboard($('dashboard-picker').value);sharing();message('Dashboard opened for this client.');});
 $('open-dashboard').onclick=()=>$('dashboard-picker').onchange();
 $('make-default').onclick=()=>attempt(async()=>{await account({op:'default',dashboard:$('dashboard-picker').value});await refresh();message('New clients will open this dashboard automatically.');});
 $('save-sharing').onclick=()=>attempt(async()=>{const d=current();if(!d)return;await account({op:'share',access:{dashboardId:d.id,owner:d.access?.owner||identity.subject,public:$('share-public').checked,viewers:[...$('share-users').selectedOptions].map(o=>o.value),expectedRevision:d.access?.revision||0,requestId:crypto.randomUUID()}});await refresh();message('Dashboard sharing saved.');});
 $('rename-client').onclick=()=>attempt(async()=>{await window.talia.renameClient($('client-name').value);message('Client name saved.');});
 $('refresh-dashboards').onclick=()=>attempt(refresh);
 window.addEventListener('talia-dashboard-loaded',()=>{const d=window.taliaDashboard;$('dashboard-title').textContent=d.id;$('dashboard-picker').value=d.id;$('client-name').value=window.talia?.registration().name||'Web dashboard';$('alerts-panel').hidden=window.taliaViewer&&!d.reads.includes('alerts');sharing();});
 await attempt(refresh);setInterval(()=>{if(!document.hidden)attempt(refresh);},10000);
}
