{async run(ctx){
 const p=ctx.params,end=ctx.now()/1000;
 // Exact instance selectors keep CPU averaging and filesystem joins host-local.
 const labels='job='+JSON.stringify(p.job)+',instance='+JSON.stringify(p.instance);
 const cpu='100 * (1 - avg(rate(node_cpu_seconds_total{'+labels+',mode="idle"}[5m])))';
 const memory='100 * (1 - node_memory_MemAvailable_bytes{'+labels+'} / node_memory_MemTotal_bytes{'+labels+'})';
 const fs=labels+',fstype!~"tmpfs|devtmpfs|overlay|squashfs"';
 const disk='100 * node_filesystem_avail_bytes{'+fs+'} / node_filesystem_size_bytes{'+fs+'}';
 const queries=[cpu,memory,disk,'up{'+labels+'}'];
 const r=await Promise.all([...queries.map(query=>ctx.source('prom',{kind:'query',query})),...[cpu,memory].map(query=>ctx.source('prom',{kind:'range',query,start:end-3600,end,step:300}))]);
 const number=s=>s&&s.samples.length&&Number.isFinite(s.samples[0][1])?s.samples[0][1]:null;
 const reachable=number(r[3].result[0])===1;
 const history=result=>(result.result[0]?.samples||[]).map(s=>Number.isFinite(s[1])?Math.round(s[1]*10)/10:null);
 const disks=r[2].result.map(s=>({id:s.metric.device+'|'+s.metric.mountpoint,mount:s.metric.mountpoint,device:s.metric.device,free:reachable?number(s):null})).sort((a,b)=>a.mount.localeCompare(b.mount));
 await ctx.publish('summary',{host:p.host,updated:ctx.now(),reachable,cpu:reachable?number(r[0].result[0]):null,memory:reachable?number(r[1].result[0]):null,disks,cpuHistory:history(r[4]),memoryHistory:history(r[5])});
}}
