(sample,host,range='day')=>{
 const v=sample.value||{},good=sample.quality==='good',up=v.reachable===true;
 const pct=n=>Number.isFinite(n)?n.toFixed(1)+'%':'Unavailable';
 const metric=(label,n,history,detail,note,chartLabel,startLabel,hasRange=false)=>({label,value:pct(n),history:Array.isArray(history)?history:[],detail,note,chartLabel,startLabel,hasRange,tone:!good||!up||!Number.isFinite(n)?'muted':n>=90?'warning':'neutral'});
 const bytes=n=>{
  const units=['B','KB','MB','GB','TB','PB'];let i=0;
  while(n>=1000&&i<units.length-1){n/=1000;i++;}
  return (i===0?String(Math.round(n)):n.toFixed(1))+' '+units[i];
 };
 const total=v.memoryTotalBytes,used=v.memoryUsedBytes,available=v.memoryAvailableBytes;
 const memoryKnown=Number.isFinite(total)&&total>0&&Number.isFinite(used)&&used>=0&&Number.isFinite(available)&&available>=0;
 const divisor=total>=1073741824?1073741824:1048576,unit=total>=1073741824?'GiB':'MiB';
 const amount=n=>(n/divisor).toFixed(1);
 const memoryDetail=memoryKnown?'Used '+amount(used)+' of '+amount(total)+' '+unit+' · '+amount(available)+' '+unit+' available':'Absolute usage unavailable';
 return {host,
  status:!good?'Collection '+sample.quality+' · showing last known readings':up?'Host metrics reachable':'Host metrics unreachable',
  statusTone:!good?'warning':up?'success':'error',
  updated:Number.isFinite(v.updated)?'Last sample: '+new Date(v.updated).toISOString().replace('T',' ').replace(/\.\d{3}Z$/,' UTC')+' · refreshes every 30s':'No collection available yet',
  cpu:{...metric('CPU usage',v.cpu,range==='hour'?v.cpuMinuteHistory:v.cpuHistory,'Current: 5-minute average across cores','Dashed line: 90% high usage',range==='hour'?'Past hour · 1-minute CPU averages':'Past 24 hours · 5-minute CPU averages',range==='hour'?'1h ago':'24h ago',true),daySelected:range==='day',hourSelected:range==='hour'},
  memory:metric('Memory usage',v.memory,v.memoryHistory,memoryDetail,'Used = total − available (Linux estimate)','Past 24 hours · 5-minute readings','24h ago'),
  disks:(v.disks||[]).filter(d=>!/^\/(boot|efi)(\/|$)/.test(d.mount)).map(d=>({id:d.id,mount:d.mount,device:d.device,value:pct(d.free),detail:Number.isFinite(d.availableBytes)&&d.availableBytes>=0&&Number.isFinite(d.totalBytes)&&d.totalBytes>0&&d.availableBytes<=d.totalBytes?bytes(d.availableBytes)+' available of '+bytes(d.totalBytes):'Absolute capacity unavailable',tone:!good||!up||!Number.isFinite(d.free)?'muted':d.free<10?'warning':'neutral'}))
 };
}
