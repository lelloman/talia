ctx => {
  const configs = [
    {name:'Homelab', instance:'node-exporter:9100', job:'node-exporter'},
    {name:'VPS-EU', instance:'vps-eu', job:'node-exporter-vps'},
    {name:'VPS-US', instance:'vps-us', job:'node-exporter-vps'}
  ];
  const data = {};
  for (const id of ['reachability','cpu','memory','disks','availability','failed','systemd']) {
    const step = ctx.steps[id];
    data[id] = step.status === 'succeeded' ? ctx.decode(step.value.wire) : null;
  }
  const sample = s => s && Number.isFinite(s.samples[0]?.[1]) ? s.samples[0][1] : null;
  const rows = (id, h) => (data[id]?.result || []).filter(s => s.metric.instance === h.instance && s.metric.job === h.job);

  const hosts = configs.map(h => {
    const problems = [];
    const add = (severity, text) => problems.push({severity, text});
    const up = sample(rows('reachability',h)[0]);
    if (up === 0) add('ERR','Host monitoring is unreachable; host may be offline.');
    else if (up !== 1) add('WARN','Host reachability data is missing or stale.');
    if (up === 1) {
      for (const [id,label] of [['cpu','CPU usage'],['memory','Memory usage']]) {
        const n = sample(rows(id,h)[0]);
        if (n === null) add('WARN',label+' is unavailable.');
        else if (n >= 90) add('WARN',label+' '+n.toFixed(1)+'% (threshold: 90%).');
      }
      const disks = rows('disks',h);
      const expected = h.name === 'Homelab' ? ['/','/boot/efi','/mnt/external'] : ['/'];
      for (const mount of expected) if (!disks.some(s => s.metric.mountpoint === mount)) add('WARN','Disk '+mount+': free-space reading unavailable.');
      for (const d of disks) {
        const n = sample(d);
        if (n === null) add('WARN','Disk '+d.metric.mountpoint+': free-space reading unavailable.');
        else if (n < 10) add('WARN','Disk '+d.metric.mountpoint+': '+n.toFixed(1)+'% free (minimum: 10%).');
      }
      if (h.job === 'node-exporter-vps') {
        if (!(sample(rows('systemd',h)[0]) > 0) || !data.failed) add('WARN','Systemd service status is unavailable.');
        else for (const s of rows('failed',h)) if (sample(s) === 1) add('ERR','Service '+s.metric.name+' is in failed state.');
      }
    }
    const availability = sample(rows('availability',h)[0]);
    if (availability === null) add('WARN','24h host scrape availability is unavailable.');
    else if (availability < 0.99) add('WARN','24h host scrape availability '+(availability*100).toFixed(2)+'% (minimum: 99%).');
    for (const [id, result] of Object.entries(data)) {
      if ((id === 'failed' || id === 'systemd') && h.name === 'Homelab') continue;
      for (const warning of result?.warnings || []) add('WARN',id+' data warning: '+warning);
    }
    return {id:h.instance, name:h.name, state:problems.some(p => p.severity === 'ERR') ? 'ERR' : problems.length ? 'WARN' : 'OK', problems};
  });
  const items = [...hosts];
  const stepData = id => ctx.steps[id]?.status === 'succeeded' ? ctx.decode(ctx.steps[id].value.wire) : null;
  const signals = stepData('signals');
  const extras = stepData('extras');
  const metricRows = (result, name, labels={}) => (result?.result || []).filter(s => s.metric.__name__ === name && Object.entries(labels).every(([k,v]) => s.metric[k] === v));
  const extra = (name, labels={}) => sample(metricRows(extras,name,labels)[0]);
  const add = (item,severity,text) => item.problems.push({severity,text});
  // A probe must identify its origin host. A probe from Talìa cannot prove VPS egress.
  for (const h of hosts) {
    if (h.problems.some(p => p.text.startsWith('Host monitoring') || p.text.startsWith('Host reachability'))) continue;
    for (const [kind,label] of [['dns','External DNS resolution'],['https','External HTTPS connection']]) {
      const n = extra('talia_report_probe_success',{host:h.name,kind});
      const checked = extra('talia_report_probe_checked_timestamp_seconds',{host:h.name,kind});
      if (n === null || checked === null || ctx.now/1000-checked > 120 || checked > ctx.now/1000+30) add(h,'WARN',label+': probe result is missing or older than 120 seconds.');
      else if (n !== 1) add(h,'WARN',label+': probe failed.');
    }
  }
  const services = [
    {id:'lelloauth',name:'LelloAuth',job:'lelloauth',instance:'lelloauth:8080'},
    {id:'dns',name:'Knot Resolver',job:'knot-resolver',instance:'knot-resolver:8453'},
    {id:'pezzottify',name:'Pezzottify',job:'pezzottify-server',instance:'pezzottify-server:9091'},
    {id:'pezzottflix',name:'Pezzottflix',job:'pezzottflix',instance:'pezzottflix:9092'},
    {id:'simple-ai',name:'SimpleAI',job:'simple-ai',instance:'simple-ai:8080'},
    {id:'simple-agents',name:'Simple Agents',job:'simple-agents',instance:'simple-agents:9781'},
    {id:'lellostore',name:'LelloStore',http:'lellostore'},
    {id:'crumbles',name:'Crumbles',http:'crumbles'}
  ];
  for (const service of services) {
    const item = {id:service.id,name:service.name,problems:[]};
    if (service.http) {
      const step = ctx.steps[service.http];
      if (step?.status !== 'succeeded') add(item,'ERR','Health endpoint check failed'+(step?.error ? ': '+step.error : '.') );
      else {
        const value = stepData(service.http);
        if (!value || !['ok','healthy'].includes(String(value.status).toLowerCase())) add(item,'WARN','Health endpoint returned an unexpected status.');
      }
    } else {
      const up = sample(rows('reachability',service)[0]);
      if (up === 0) add(item,'ERR','Service metrics endpoint is unreachable.');
      else if (up !== 1) add(item,'WARN','Service reachability data is missing or stale.');
      const a = sample(rows('availability',service)[0]);
      if (a === null) add(item,'WARN','24h scrape availability is unavailable.');
      else if (a < 0.99) add(item,'WARN','24h scrape availability '+(a*100).toFixed(2)+'% (minimum: 99%).');
    }
    for (const warning of data.reachability?.warnings || []) if (!service.http) add(item,'WARN','Reachability data warning: '+warning);
    for (const warning of data.availability?.warnings || []) if (!service.http) add(item,'WARN','Availability data warning: '+warning);
    items.push(item);
  }
  const byId = id => items.find(i => i.id === id);
  for (const [id,metric,limit,label] of [
    ['dns','talia_dns_servfail_ratio',0.01,'DNS SERVFAIL ratio over 15m'],
    ['pezzottify','talia_music_error_ratio',0.01,'HTTP 5xx ratio over 15m']
  ]) {
    const n = sample(metricRows(signals,metric)[0]);
    if (n === null) add(byId(id),'WARN',label+': unavailable.');
    else if (n > limit) add(byId(id),'WARN',label+': '+(n*100).toFixed(2)+'% (maximum: 1%).');
  }
  const ai = sample(metricRows(signals,'simpleai_up')[0]);
  if (ai === null) add(byId('simple-ai'),'WARN','SimpleAI health metric unavailable.');
  else if (ai !== 1) add(byId('simple-ai'),'ERR','SimpleAI reports unhealthy.');
  for (const metric of ['simple_agents_uncertain_effects','simple_agents_exhausted_recovery']) {
    const results = metricRows(signals,metric);
    if (!results.length) add(byId('simple-agents'),'WARN',metric+': unavailable.');
    else { const n=results.reduce((sum,r)=>sum+(sample(r)||0),0); if(n>0) add(byId('simple-agents'),'WARN',metric+': '+n+'.'); }
  }
  if (!signals) for (const id of ['dns','pezzottify','simple-ai','simple-agents']) add(byId(id),'WARN','Additional service checks could not be collected.');
  for (const warning of signals?.warnings || []) for (const id of ['dns','pezzottify','simple-ai','simple-agents']) add(byId(id),'WARN','Service data warning: '+warning);
  for (const warning of extras?.warnings || []) for(const item of hosts) add(item,'WARN','Probe data warning: '+warning);
  for (const item of items) item.state=item.problems.some(p=>p.severity==='ERR')?'ERR':item.problems.length?'WARN':'OK';
  return {items,state:items.some(i=>i.state==='ERR')?'Error':items.some(i=>i.state==='WARN')?'Warning':'Nominal'};
}
