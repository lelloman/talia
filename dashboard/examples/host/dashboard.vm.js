defineVM({
 initial:()=>({ready:false,presentation:{},sample:null,cpuRange:'day'}),
 async start(ctx){await ctx.subscribe(ctx.params.summary,'snapshot');},
 actions:{
  snapshot(ctx,event){const s=ctx.state();ctx.commit(s,{ready:true,sample:event.value,cpuRange:s.value.cpuRange,presentation:ctx.fn('host-present',event.value,ctx.params.host,s.value.cpuRange)});},
  cpuDay(ctx){const s=ctx.state();ctx.commit(s,{...s.value,cpuRange:'day',presentation:ctx.fn('host-present',s.value.sample,ctx.params.host,'day')});},
  cpuHour(ctx){const s=ctx.state();ctx.commit(s,{...s.value,cpuRange:'hour',presentation:ctx.fn('host-present',s.value.sample,ctx.params.host,'hour')});}
 }
});
