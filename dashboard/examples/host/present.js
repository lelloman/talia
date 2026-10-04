(sample,host,range='day',now=Date.now())=>{
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
 return {host,repositories,
  status:!good?'Collection '+sample.quality+' · showing last known readings':up?'Host metrics reachable':'Host metrics unreachable',
  statusTone:!good?'warning':up?'success':'error',
  updated:Number.isFinite(v.updated)?'Last sample: '+new Date(v.updated).toISOString().replace('T',' ').replace(/\.\d{3}Z$/,' UTC')+' · refreshes every 30s':'No collection available yet',
  cpu:{...metric('CPU usage',v.cpu,range==='hour'?v.cpuMinuteHistory:v.cpuHistory,'Current: 5-minute average across cores','Dashed line: 90% high usage',range==='hour'?'Past hour · 1-minute CPU averages':'Past 24 hours · 5-minute CPU averages',range==='hour'?'1h ago':'24h ago',true),daySelected:range==='day',hourSelected:range==='hour'},
  memory:metric('Memory usage',v.memory,v.memoryHistory,memoryDetail,'Used = total − available (Linux estimate)','Past 24 hours · 5-minute readings','24h ago'),
  disks:(v.disks||[]).filter(d=>!/^\/(boot|efi)(\/|$)/.test(d.mount)).map(d=>({id:d.id,mount:d.mount,device:d.device,value:pct(d.free),detail:Number.isFinite(d.availableBytes)&&d.availableBytes>=0&&Number.isFinite(d.totalBytes)&&d.totalBytes>0&&d.availableBytes<=d.totalBytes?bytes(d.availableBytes)+' available of '+bytes(d.totalBytes):'Absolute capacity unavailable',tone:!good||!up||!Number.isFinite(d.free)?'muted':d.free<10?'warning':'neutral'}))
 };
}
