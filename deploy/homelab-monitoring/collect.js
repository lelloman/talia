{async run(ctx){
 const result=await ctx.source('prom',{kind:'query',query:ctx.params.query});
 if(result.resultType!=='vector'||result.warnings?.length)throw Error('Incomplete Prometheus query');
 if(result.result.length>100)throw Error('Alert series limit exceeded');
 const series=result.result.map(s=>{
  const labels={...s.metric};delete labels.__name__;
  const value=s.samples?.[0]?.[1];
  if(!Number.isFinite(value))throw Error('Invalid metric value');
  return {labels,value};
 });
 await ctx.publish('result',{series,checked:ctx.now()});
}}
