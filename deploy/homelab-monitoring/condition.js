{async evaluate(ctx){
 const s=await ctx.read('result'),p=ctx.params,now=ctx.now();
 if(!s.hasValue||s.quality!=='good'||now-s.timestamp>p.staleMs)throw Error('Alert measurement unavailable');
 const identity=labels=>JSON.stringify(Object.keys(labels).sort().map(k=>[k,labels[k]]));
 const previous=ctx.state.pending||{},pending={},firing=[];
 const continuous=Number.isFinite(ctx.state.checked)&&s.value.checked-ctx.state.checked<=p.staleMs;
 for(const item of s.value.series){
  const key=identity(item.labels);
  pending[key]=continuous&&previous[key]!==undefined?previous[key]:s.value.checked;
  if(s.value.checked-pending[key]>=p.forMs)firing.push({...item,key});
 }
 ctx.state.pending=pending;ctx.state.checked=s.value.checked;
 firing.sort((a,b)=>a.key.localeCompare(b.key));
 const members=firing.map(s=>s.key).join('\n');
 if(members&&members!==ctx.state.members)ctx.state.stage=ctx.state.stage==='firing-a'?'firing-b':'firing-a';
 ctx.state.members=members;
 const format=(text,item)=>text.replace(/{{\s*\$labels\.(\w+)\s*}}/g,(_,key)=>item.labels[key]||'')
  .replace(/{{\s*printf\s+"%\.1f"\s+\$value\s*}}/g,item.value.toFixed(1))
  .replace(/{{\s*\$value\s*\|\s*humanize\s*}}/g,String(item.value))
  .replace(/{{\s*\$value\s*}}/g,String(item.value));
 let message=p.name+': recovered';
 if(firing.length)message=p.name+' ('+firing.length+' affected)\n'+firing.map(item=>
  format(p.summary,item)+'\n'+format(p.description,item)+'\n'+Object.entries(item.labels).map(([k,v])=>k+'='+v).join(', ')).join('\n\n');
 // Keep below both the engine's UTF-8 message bound and Telegram's message bound.
 if(message.length>1000)message=message.slice(0,950)+'\n… See Talìa monitoring input for all affected series.';
 return {active:!!firing.length,stage:ctx.state.stage||'firing-a',severity:p.severity,message};
}}
