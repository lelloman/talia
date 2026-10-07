(sample,host,range='day',now=Date.now())=>{
 const v=sample.value||{},good=sample.quality==='good',up=v.reachable===true;
 const pct=n=>Number.isFinite(n)?n.toFixed(1)+'%':'Unavailable';
 const metric=(label,n,history,detail,note,chartLabel,startLabel,hasRange=false)=>({peakHistory:[],label,value:pct(n),history:Array.isArray(history)?history:[],detail,note,chartLabel,startLabel,hasRange,tone:!good||!up||!Number.isFinite(n)?'muted':n>=90?'warning':'neutral'});
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
 const git=v.gitWorkspaces;
 let repositories={visible:!!git,title:'',tone:'warning',summary:'',checked:'',lastClean:''};
 if(git){
  const date=n=>new Date(n*1000).toISOString().replace('T',' ').replace(/\.\d{3}Z$/,' UTC');
  const validTime=Number.isFinite(git.checked)&&git.checked>0&&git.checked<=now/1000+30;
  const fresh=validTime&&now/1000-git.checked<=86700;
  const dirty=git.dirty||[],errors=git.errors||[];
  const clean=good&&up&&fresh&&git.success===1&&git.repositories>0&&git.worktrees>0&&!git.dataWarning&&!dirty.length&&!errors.length;
  const paths=values=>values.slice(0,4).map(p=>p.replace(/^\/home\/[^/]+\//,'~/')).join(', ')+(values.length>4?' … +'+(values.length-4)+' more':'');
  const problems=[];
  if(!good||!up)problems.push('Host monitoring unavailable');
  if(!fresh)problems.push(validTime?'Daily check overdue':'No recent check available');
  if(dirty.length)problems.push(dirty.length+' dirty worktree'+(dirty.length===1?'':'s')+': '+paths(dirty));
  if(errors.length)problems.push('Could not check: '+paths(errors));
  if(git.success!==1||!git.repositories||!git.worktrees||git.dataWarning)problems.push('Repository check incomplete');
  repositories={visible:true,title:clean?'✅ Git worktrees clean':'⚠ Git worktrees need attention',tone:clean?'success':'warning',
   summary:clean?git.repositories+' repositories · '+git.worktrees+' worktrees · checked every 24 hours':problems.join(' · '),
   checked:validTime?'Last check: '+date(git.checked):'No completed check',
   lastClean:Number.isFinite(git.lastClean)&&git.lastClean>0&&git.lastClean<=now/1000+30?'Last successful clean check: '+date(git.lastClean):'No successful clean check recorded'};
 }
 repositories.canCheck=false;
 return {host,repositories,network:networkPresentation(v.network),
  status:!good?'Collection '+sample.quality+' · showing last known readings':up?'Host metrics reachable':'Host metrics unreachable',
  statusTone:!good?'warning':up?'success':'error',
  updated:Number.isFinite(v.updated)?'Last sample: '+new Date(v.updated).toISOString().replace('T',' ').replace(/\.\d{3}Z$/,' UTC')+' · refreshes every 30s':'No collection available yet',
  cpu:{...metric('CPU usage',v.cpu,range==='hour'?v.cpuMinuteHistory:v.cpuHistory,'Current: 5-minute average across cores','',range==='hour'?'Past hour · 1-minute CPU averages':'Past 24 hours · 5-minute CPU averages',range==='hour'?'1h ago':'24h ago',true),peakHistory:(range==='hour'?v.cpuMinutePeakHistory:v.cpuPeakHistory)||Array((range==='hour'?v.cpuMinuteHistory:v.cpuHistory)?.length||0).fill(null),note:'Max uses ~30-second samples averaged across cores',daySelected:range==='day',hourSelected:range==='hour'},
  memory:metric('Memory usage',v.memory,v.memoryHistory,memoryDetail,'Used = total − available (Linux estimate)','Past 24 hours · 5-minute readings','24h ago'),
  disks:(v.disks||[]).filter(d=>!/^\/(boot|efi)(\/|$)/.test(d.mount)).map(d=>disk(d))
 };
 function networkPresentation(n){
  n=n||{};
  const known=good&&up&&n.fresh&&!n.unavailable;
  const download=Array.isArray(n.downloadHistory)?n.downloadHistory:[],upload=Array.isArray(n.uploadHistory)?n.uploadHistory:[];
  const highest=Math.max(1,...download.filter(Number.isFinite),...upload.filter(Number.isFinite));
  const units=['B/s','KB/s','MB/s','GB/s'];let index=0,factor=1;
  while(highest/factor>=1000&&index<units.length-1){index++;factor*=1000;}
  const magnitude=Math.pow(10,Math.floor(Math.log10(highest/factor))),normalized=highest/factor/magnitude;
  const chartMax=([1,2,5,10].find(n=>n>=normalized)||10)*magnitude;
  const speed=n=>known&&Number.isFinite(n)&&n>=0?bytes(n)+'/s':'Unavailable';
  const totals=[1,7,30].map(days=>{
   const t=(n.totals||[]).find(t=>t.days===days)||{};
   const valid=Number.isFinite(t.download)&&t.download>=0&&Number.isFinite(t.upload)&&t.upload>=0;
   const observed=Number.isFinite(t.observedSeconds)?t.observedSeconds:0;
   const complete=observed>=days*86400*.995&&!t.warning;
   const duration=observed<3600?'less than 1 hour':observed<86400?(observed/3600).toFixed(1)+' hours':(observed/86400).toFixed(1)+' days';
   return {id:String(days),label:days===1?'Last 24 hours':'Last '+days+' days',download:valid?'↓ '+bytes(t.download):'↓ Unavailable',upload:valid?'↑ '+bytes(t.upload):'↑ Unavailable',coverage:!valid?'No transfer history':complete?'Complete history':'Partial history · '+duration+' sampled',tone:!valid||!complete?'muted':'neutral'};
  });
  return {download:speed(n.download),upload:speed(n.upload),downloadHistory:download.map(x=>Number.isFinite(x)?x/factor:null),uploadHistory:upload.map(x=>Number.isFinite(x)?x/factor:null),chartMax,unit:units[index],totals,
   detail:n.device?'Interface: '+n.device+' · host traffic':'Network interface not configured',
   note:!known?'Current network readings unavailable · retained history shown':n.warning?'Network data has source warnings':'Speeds sampled every 30s · transfer totals are estimates',
   chartLabel:'Upload and download · past 60 minutes'};
 }
 function disk(d){
  const known=Number.isFinite(d.free)&&good&&up,item={id:d.id,mount:d.mount,device:d.device,value:pct(d.free),detail:Number.isFinite(d.availableBytes)&&d.availableBytes>=0&&Number.isFinite(d.totalBytes)&&d.totalBytes>0&&d.availableBytes<=d.totalBytes?bytes(d.availableBytes)+' available of '+bytes(d.totalBytes):'Absolute capacity unavailable',tone:!good||!up||!Number.isFinite(d.free)?'muted':d.free<10?'warning':'neutral'};
  return {...item,free:Number.isFinite(d.free)?pct(d.free)+' free':'Unavailable',used:known?100-d.free:null,meterLabel:d.mount+' used space',summary:item.detail+(d.device?' · '+d.device:'')};
 }
}
