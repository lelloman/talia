{async evaluate(ctx){
 const s=await ctx.read('result'),p=ctx.params,now=ctx.now();
 const stale=!s.hasValue||s.quality!=='good'||now-s.timestamp>p.staleMs;
 if(stale&&ctx.state.since===undefined)ctx.state.since=now;
 if(!stale)delete ctx.state.since;
 return {active:stale&&now-ctx.state.since>=120000,stage:'firing-a',severity:'warning',
  message:p.name+': '+(stale?'monitoring input unavailable; condition cannot be evaluated':'monitoring input recovered')};
}}
