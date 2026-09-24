(sample,host)=>{
 const v=sample.value||{},good=sample.quality==='good',up=v.reachable===true;
 const pct=n=>Number.isFinite(n)?n.toFixed(1)+'%':'Unavailable';
 const metric=(label,n,history,detail,note)=>({label,value:pct(n),history:Array.isArray(history)?history:[],detail,note,tone:!good||!up||!Number.isFinite(n)?'muted':n>=90?'warning':'neutral'});
 const total=v.memoryTotalBytes,used=v.memoryUsedBytes,available=v.memoryAvailableBytes;
 const memoryKnown=Number.isFinite(total)&&total>0&&Number.isFinite(used)&&used>=0&&Number.isFinite(available)&&available>=0;
 const divisor=total>=1073741824?1073741824:1048576,unit=total>=1073741824?'GiB':'MiB';
 const amount=n=>(n/divisor).toFixed(1);
 const memoryDetail=memoryKnown?'Used '+amount(used)+' of '+amount(total)+' '+unit+' · '+amount(available)+' '+unit+' available':'Absolute usage unavailable';
 return {host,
  status:!good?'Collection '+sample.quality+' · showing last known readings':up?'Host metrics reachable':'Host metrics unreachable',
  statusTone:!good?'warning':up?'success':'error',
  updated:Number.isFinite(v.updated)?'Last sample: '+new Date(v.updated).toISOString().replace('T',' ').replace(/\.\d{3}Z$/,' UTC')+' · refreshes every 30s':'No collection available yet',
  cpu:metric('CPU usage',v.cpu,v.cpuHistory,'5-minute average across cores','Dashed line: 90% high usage'),memory:metric('Memory usage',v.memory,v.memoryHistory,memoryDetail,'Used = total − available (Linux estimate)'),
  disks:(v.disks||[]).map(d=>({id:d.id,mount:d.mount,device:d.device,value:pct(d.free),tone:!good||!up||!Number.isFinite(d.free)?'muted':d.free<10?'warning':'neutral'}))
 };
}
