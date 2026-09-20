defineVM({
  initial: () => ({title:{text:'Talìa · monitoring'},explanation:{text:'Writes change server state and survive dashboard reload.'},screen:'overviewScreen', value:25, details:true, busy:false,
    status:'Connecting…', history:[], services:[{id:'engine',label:'Engine connection'}]}),
  async start(ctx) { await ctx.subscribe('value', 'snapshot'); },
  resume(ctx) { const s=ctx.state(); ctx.commit(s,{...s.value,busy:false}); },
  actions: {
    overview(ctx) { const s=ctx.state(); ctx.commit(s,{...s.value,screen:'overviewScreen'}); },
    controls(ctx) { const s=ctx.state(); ctx.commit(s,{...s.value,screen:'controlsScreen'}); },
    setValue(ctx,event) { const s=ctx.state(); ctx.commit(s,{...s.value,value:event.value}); },
    details(ctx,event) { const s=ctx.state(); ctx.commit(s,{...s.value,details:event.value}); },
    snapshot(ctx,event) {
      const s=ctx.state(), sample=event.value;
      ctx.commit(s,{...s.value,status:'Server revision '+sample.revision,
        history:[...s.value.history,sample.value].slice(-30)});
    },
    async apply(ctx) {
      let s=ctx.state(); const value=s.value.value;
      ctx.commit(s,{...s.value,busy:true});
      try { await ctx.write(value); }
      catch(error) { s=ctx.state(); ctx.commit(s,{...s.value,status:String(error)}); }
      finally { s=ctx.state(); ctx.commit(s,{...s.value,busy:false}); }
    }
  }
});
