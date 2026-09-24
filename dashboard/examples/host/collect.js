{async run(ctx){
 const p=ctx.params,end=ctx.now()/1000,start=end-86400,step=300,hourStart=end-3600;
 // Exact instance selectors keep CPU averaging and filesystem joins host-local.
 const labels='job='+JSON.stringify(p.job)+',instance='+JSON.stringify(p.instance);
 const cpu='100 * (1 - avg(rate(node_cpu_seconds_total{'+labels+',mode="idle"}[5m])))';
 const cpuMinute='100 * (1 - avg(rate(node_cpu_seconds_total{'+labels+',mode="idle"}[1m])))';
 const memory='100 * (1 - node_memory_MemAvailable_bytes{'+labels+'} / node_memory_MemTotal_bytes{'+labels+'})';
 const memoryTotal='node_memory_MemTotal_bytes{'+labels+'}',memoryAvailable='node_memory_MemAvailable_bytes{'+labels+'}';
 const fs=labels+',fstype!~"tmpfs|devtmpfs|overlay|squashfs"';
 const disk='100 * node_filesystem_avail_bytes{'+fs+'} / node_filesystem_size_bytes{'+fs+'}';
 const queries=[cpu,memoryTotal,memoryAvailable,disk,'up{'+labels+'}'];
 const r=await Promise.all([...queries.map(query=>ctx.source('prom',{kind:'query',query})),...[cpu,memory].map(query=>ctx.source('prom',{kind:'range',query,start,end,step})),ctx.source('prom',{kind:'range',query:cpuMinute,start:hourStart,end,step:60})]);
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
 const disks=r[3].result.map(s=>({id:s.metric.device+'|'+s.metric.mountpoint,mount:s.metric.mountpoint,device:s.metric.device,free:reachable?number(s):null})).sort((a,b)=>a.mount.localeCompare(b.mount));
 await ctx.publish('summary',{host:p.host,updated:ctx.now(),reachable,cpu:reachable?number(r[0].result[0]):null,memory:validMemory?100*used/total:null,memoryUsedBytes:used,memoryTotalBytes:validMemory?total:null,memoryAvailableBytes:validMemory?available:null,disks,cpuHistory:history(r[5],start,step),memoryHistory:history(r[6],start,step),cpuMinuteHistory:history(r[7],hourStart,60)});
}}
