{async run(ctx){
 const p=ctx.params,end=ctx.now()/1000,start=end-86400,step=300,hourStart=end-3600;
 // Exact instance selectors keep CPU averaging and filesystem joins host-local.
 const labels='job='+JSON.stringify(p.job)+',instance='+JSON.stringify(p.instance);
 const cpu='100 * (1 - avg(rate(node_cpu_seconds_total{'+labels+',mode="idle"}[5m])))';
 const cpuMinute='100 * (1 - avg(rate(node_cpu_seconds_total{'+labels+',mode="idle"}[1m])))';
 // irate uses the latest two scrapes; the subquery retains peaks at 30-second resolution.
 const cpuFast='100 * (1 - avg(irate(node_cpu_seconds_total{'+labels+',mode="idle"}[2m])))';
 const peak=window=>'max_over_time(('+cpuFast+')['+window+':30s])';
 const memory='100 * (1 - node_memory_MemAvailable_bytes{'+labels+'} / node_memory_MemTotal_bytes{'+labels+'})';
 const memoryTotal='node_memory_MemTotal_bytes{'+labels+'}',memoryAvailable='node_memory_MemAvailable_bytes{'+labels+'}';
 const fs=labels+',fstype!~"tmpfs|devtmpfs|overlay|squashfs"';
 const disk='node_filesystem_avail_bytes{'+fs+'}',diskTotal='node_filesystem_size_bytes{'+fs+'}';
 const queries=[cpu,memoryTotal,memoryAvailable,disk,'up{'+labels+'}'];
 const r=await Promise.all([...queries.map(query=>ctx.source('prom',{kind:'query',query})),...[cpu,memory].map(query=>ctx.source('prom',{kind:'range',query,start,end,step})),ctx.source('prom',{kind:'range',query:cpuMinute,start:hourStart,end,step:60}),ctx.source('prom',{kind:'query',query:diskTotal})]);
 const peaks=await Promise.all([{start,step,window:'5m'},{start:hourStart,step:60,window:'1m'}].map(q=>ctx.source('prom',{kind:'range',query:peak(q.window),start:q.start,end,step:q.step})));
 const number=s=>s&&s.samples.length&&Number.isFinite(s.samples[0][1])?s.samples[0][1]:null;
 const reachable=number(r[4].result[0])===1;
 const total=number(r[1].result[0]),available=number(r[2].result[0]);
 const validMemory=reachable&&total!==null&&total>0&&available!==null&&available>=0&&available<=total;
 const used=validMemory?total-available:null;
 const history=(result,begin,interval)=>{
  const values=Array(Math.round((end-begin)/interval)+1).fill(null);
  for(const [timestamp,value] of result.result[0]?.samples||[]){
   const i=Math.round((timestamp-begin)/interval);
   if(i>=0&&i<values.length&&Number.isFinite(value))values[i]=Math.round(value*10)/10;
  }
  return values;
 };
 const diskKey=s=>s.metric.device+'|'+s.metric.mountpoint;
 const sizes=new Map(r[8].result.map(s=>[diskKey(s),number(s)]));
 const disks=r[3].result.filter(s=>!/^\/(boot|efi)(\/|$)/.test(s.metric.mountpoint)).map(s=>{
  const totalBytes=sizes.get(diskKey(s)),availableBytes=number(s);
  const valid=reachable&&Number.isFinite(totalBytes)&&totalBytes>0&&availableBytes!==null&&availableBytes>=0&&availableBytes<=totalBytes;
  return {id:diskKey(s),mount:s.metric.mountpoint,device:s.metric.device,free:valid?100*availableBytes/totalBytes:null,availableBytes:valid?availableBytes:null,totalBytes:valid?totalBytes:null};
 }).sort((a,b)=>a.mount.localeCompare(b.mount));
 let gitWorkspaces=null;
 if(p.gitWorkspaces){
  try {
   const result=await ctx.source('prom',{kind:'query',query:'{'+labels+',host='+JSON.stringify(p.host)+',__name__=~"talia_git_.*"}'});
   const metric=name=>number(result.result.find(s=>s.metric.__name__==='talia_git_'+name));
   gitWorkspaces={automatic:metric('automatic'),checked:metric('checked_timestamp_seconds'),lastClean:metric('last_clean_timestamp_seconds'),success:metric('success'),repositories:metric('repositories'),
    dirty:result.result.filter(s=>s.metric.__name__==='talia_git_worktree_dirty'&&number(s)===1).map(s=>s.metric.path),
    errors:result.result.filter(s=>s.metric.__name__==='talia_git_worktree_error'&&number(s)===1).map(s=>s.metric.path),
    worktrees:result.result.filter(s=>s.metric.__name__==='talia_git_worktree_dirty').length,
    dataWarning:!!result.warnings?.length};
  } catch (_) { gitWorkspaces={unavailable:true}; }
 }
 let network={unavailable:true};
 if(p.networkDevice){
  const netLabels=labels+',device='+JSON.stringify(p.networkDevice);
  const prefix=p.networkHostProbe?'talia_host_network_':'node_network_';
  const rx=prefix+'receive_bytes_total{'+netLabels+'}',tx=prefix+'transmit_bytes_total{'+netLabels+'}';
  const direction=(query,name)=>'label_replace('+query+',"direction","'+name+'","","")';
  try {
   const speed=direction('sum(irate('+rx+'[2m]))','download')+' or '+direction('sum(irate('+tx+'[2m]))','upload');
   const graph=await ctx.source('prom',{kind:'range',query:speed,start:hourStart,end,step:30});
   const checked=p.networkHostProbe?number((await ctx.source('prom',{kind:'query',query:'talia_host_network_checked_timestamp_seconds{'+labels+'}'})).result[0]):end;
   const fresh=checked!==null&&checked<=end+30&&end-checked<=90;
   const metricHistory=name=>history({result:graph.result.filter(s=>s.metric.direction===name)},hourStart,30);
   const downloadHistory=metricHistory('download'),uploadHistory=metricHistory('upload');
   const totals=await Promise.all([1,7,30].map(async days=>{
    const duration=days+'d';
    // Tag each direction before combining: range functions discard metric names.
    const coverage='min('+direction('count_over_time('+rx+'['+duration+'])','download')+' or '+direction('count_over_time('+tx+'['+duration+'])','upload')+')';
    const query=direction('sum(increase('+rx+'['+duration+']))','download')+' or '+direction('sum(increase('+tx+'['+duration+']))','upload')+' or '+direction(coverage,'samples');
    try {
     const result=await ctx.source('prom',{kind:'query',query});
     const get=name=>number(result.result.find(s=>s.metric.direction===name));
     return {days,download:get('download'),upload:get('upload'),observedSeconds:Math.min(days*86400,Math.max(0,(get('samples')||0)-1)*30),warning:!!result.warnings?.length};
    } catch (_) {return {days,download:null,upload:null,observedSeconds:0,warning:true};}
   }));
   network={device:p.networkDevice,fresh:reachable&&fresh,download:reachable&&fresh?downloadHistory.at(-1):null,upload:reachable&&fresh?uploadHistory.at(-1):null,downloadHistory,uploadHistory,totals,warning:!!graph.warnings?.length};
  } catch (_) {network={device:p.networkDevice,unavailable:true};}
 }
 await ctx.publish('summary',{network,gitWorkspaces,host:p.host,updated:ctx.now(),reachable,cpu:reachable?number(r[0].result[0]):null,memory:validMemory?100*used/total:null,memoryUsedBytes:used,memoryTotalBytes:validMemory?total:null,memoryAvailableBytes:validMemory?available:null,disks,cpuHistory:history(r[5],start,step),memoryHistory:history(r[6],start,step),cpuMinuteHistory:history(r[7],hourStart,60),cpuPeakHistory:history(peaks[0],start,step),cpuMinutePeakHistory:history(peaks[1],hourStart,60)});
}}
