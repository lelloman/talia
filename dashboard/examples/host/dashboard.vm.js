defineVM({
 initial:()=>({ready:false,presentation:{}}),
 async start(ctx){await ctx.subscribe(ctx.params.summary,'snapshot');},
 actions:{snapshot(ctx,event){const s=ctx.state();ctx.commit(s,{ready:true,presentation:ctx.fn('host-present',event.value,ctx.params.host)});}}
});
