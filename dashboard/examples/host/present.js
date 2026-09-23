(sample,host)=>{
 const v=sample.value||{},good=sample.quality==='good',up=v.reachable===true;
 const pct=n=>Number.isFinite(n)?n.toFixed(1)+'%':'Unavailable';
 const metric=(label,n,history)=>({label,value:pct(n),history:Array.isArray(history)?history:[],tone:!good||!up||!Number.isFinite(n)?'muted':n>=90?'warning':'neutral'});
 return {host,
  status:!good?'Collection '+sample.quality+' · showing last known readings':up?'Host metrics reachable':'Host metrics unreachable',
  statusTone:!good?'warning':up?'success':'error',
  updated:Number.isFinite(v.updated)?'Last sample: '+new Date(v.updated).toISOString().replace('T',' ').replace(/\.\d{3}Z$/,' UTC')+' · refreshes every 30s':'No collection available yet',
  cpu:metric('CPU usage',v.cpu,v.cpuHistory),memory:metric('Memory usage',v.memory,v.memoryHistory),
  disks:(v.disks||[]).map(d=>({id:d.id,mount:d.mount,device:d.device,value:pct(d.free),tone:!good||!up||!Number.isFinite(d.free)?'muted':d.free<10?'warning':'neutral'}))
 };
}
