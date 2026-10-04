function hostPresentation(ctx,state){
 const presentation=ctx.fn('host-present',state.sample,ctx.params.host,state.cpuRange);
 presentation.repositories={...presentation.repositories,canCheck:!!ctx.params.gitRecheck,checkEnabled:!state.probeBusy,checkLabel:state.probeBusy?'Checking…':'Check now',checkMessage:state.probeMessage||''};
 return presentation;
}
function updateHost(ctx,patch){
 const s=ctx.state(),next={...s.value,...patch};
 if(next.sample)next.presentation=hostPresentation(ctx,next);
 ctx.commit(s,next);
}
defineVM({
 initial:()=>({ready:false,presentation:{},sample:null,cpuRange:'day',probeBusy:false,probeMessage:'',probeRun:null}),
 async start(ctx){
  await ctx.subscribe(ctx.params.summary,'snapshot');
  if(ctx.params.gitRecheck)await ctx.subscribe('monitor.'+ctx.params.gitRecheck,'probeStatus');
 },
 actions:{
  snapshot(ctx,event){updateHost(ctx,{ready:true,sample:event.value});},
  cpuDay(ctx){updateHost(ctx,{cpuRange:'day'});},
  cpuHour(ctx){updateHost(ctx,{cpuRange:'hour'});},
  probeStatus(ctx,event){
   const run=event.value?.value?.run;
   if(!run)return;
   const state=ctx.state().value;
   if(state.probeBusy&&run.id===state.probePreviousRun&&!['pending','queued','running'].includes(run.status))return;
   const busy=['pending','queued','running'].includes(run.status);
   const message=busy?'Waiting for the host to finish the check…':run.status==='complete'?'Check completed. Results refresh within 30 seconds.':['failed','cancelled','timed_out','unknown'].includes(run.status)?'Check did not complete successfully. You can try again.':'';
   updateHost(ctx,{probeBusy:busy,probeMessage:message,probeRun:run.id});
  },
  async checkRepositories(ctx){
   if(!ctx.params.gitRecheck||ctx.state().value.probeBusy)return;
   updateHost(ctx,{probeBusy:true,probePreviousRun:ctx.state().value.probeRun,probeMessage:'Requesting a fresh check…'});
   try {
    await ctx.run(ctx.params.gitRecheck);
    // The subscribed server run drives completion, including across reloads.
    if(ctx.state().value.probeBusy)updateHost(ctx,{probeMessage:'Waiting for the host to finish the check…'});
   } catch (_) {updateHost(ctx,{probeBusy:false,probeMessage:'Could not request the check. Please try again.'});}
  }
 }
});
