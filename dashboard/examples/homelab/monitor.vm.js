defineVM({
 initial:()=>({status:'Waiting for monitoring data',statusTone:'muted',updated:'Collected every 30 seconds',summary:'Service status unavailable',
  cpu:{label:'CPU usage',value:'—',history:[],tone:'muted'},memory:{label:'Memory usage',value:'—',history:[],tone:'muted'},disks:[],targets:[]}),
 async start(ctx){await ctx.subscribe('homelab-summary','snapshot');},
 actions:{snapshot(ctx,event){
  const sample=event.value,v=sample.value,s=ctx.state();
  if(!v||!Array.isArray(v.targets)||!Array.isArray(v.disks)){ctx.commit(s,{...s.value,status:'Waiting for valid monitoring data',statusTone:'warning'});return;}
  const pct=n=>Number.isFinite(n)?n.toFixed(1)+'%':'Unavailable';
  const good=sample.quality==='good';
  const up=v.targets.filter(t=>t.status==='up').length;
  const tone=n=>!good||!Number.isFinite(n)?'muted':n>=90?'warning':'neutral';
  const targets=v.targets.map(t=>({id:t.id,name:t.name,endpoint:t.instance,status:!good?'Last known: '+t.status:t.status==='up'?'● Reachable':t.status==='down'?'● Unreachable':'○ No data',tone:!good?'muted':t.status==='up'?'success':t.status==='down'?'error':'warning',order:t.status==='up'?1:0})).sort((a,b)=>a.order-b.order||a.name.localeCompare(b.name));
  ctx.commit(s,{...s.value,
   status:!good?'Collection '+sample.quality+' · showing last known readings':up===targets.length&&targets.length>0?'All scrape targets reachable':'Some scrape targets need attention',
   statusTone:!good?'warning':up===targets.length&&targets.length>0?'success':'warning',
   updated:'Last sample: '+new Date(v.updated).toISOString().replace('T',' ').replace(/\.\d{3}Z$/,' UTC')+' · refreshes every 30s',
   summary:up+' / '+targets.length+' reachable'+(!good?' at last collection':''),
   cpu:{label:'CPU usage',value:pct(v.cpu),history:v.cpuHistory||[],tone:tone(v.cpu)},
   memory:{label:'Memory usage',value:pct(v.memory),history:v.memoryHistory||[],tone:tone(v.memory)},
   disks:v.disks.map(d=>({id:d.id,mount:d.mount,device:d.device,value:pct(d.free),tone:!good||!Number.isFinite(d.free)?'muted':d.free<10?'warning':'neutral'})),targets
  });
 }}
});
