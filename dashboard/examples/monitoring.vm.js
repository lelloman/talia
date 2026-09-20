const metricNames=['cpu','memory','disk','disk-y'];
function describe(samples){
 const lines=metricNames.map(id=>{const s=samples[id];const label={cpu:'CPU',memory:'Memory',disk:'Disk X free','disk-y':'Disk Y free'}[id];if(!s||!s.hasValue)return {id,label:label+': awaiting data'};const age=Math.max(0,Math.floor((Date.now()-s.timestamp)/1000));return {id,label:label+': '+String(s.value)+'% · '+s.quality+' · '+age+'s ago'};});
 const watch=id=>{const v=samples['monitor.disk-'+id]?.value;if(!v)return 'Disk '+id.toUpperCase()+': awaiting Watch';if(v.error)return 'Disk '+id.toUpperCase()+': '+v.error;return 'Disk '+id.toUpperCase()+': '+(v.state.low?'investigation triggered':v.state.high?'warning threshold crossed':'armed');};
 const investigation=samples['monitor.investigate']?.value,breakdown=samples.breakdown;
 return {metrics:lines,watchX:watch('x'),watchY:watch('y'),runStatus:investigation?.run?.status||'No investigation yet',hasCollectionError:!!samples['monitor.collection']?.value?.error,collectionError:samples['monitor.collection']?.value?.error||'',services:breakdown?.hasValue?Object.entries(breakdown.value).map(([id,value])=>({id,label:id+': '+String(value)+'%'})):[]};
}
defineVM({
 initial:()=>({screen:'overview',samples:{},metrics:metricNames.map(id=>({id,label:id+': awaiting data'})),watchX:'Awaiting Watch X',watchY:'Awaiting Watch Y',runStatus:'No investigation yet',collectionError:'',hasCollectionError:false,services:[],busy:false,message:'',history:[]}),
 async start(ctx){for(const id of [...metricNames,'breakdown','monitor.disk-x','monitor.disk-y','monitor.investigate','monitor.collection'])await ctx.subscribe(id,'sample');},
 resume(ctx){const s=ctx.state();ctx.commit(s,{...s.value,busy:false});},
 actions:{
  sample(ctx,event){const s=ctx.state(),v=event.value,samples={...s.value.samples,[v.id]:v};ctx.commit(s,{...s.value,samples,...describe(samples),history:v.id==='cpu'?[...s.value.history,v.value].slice(-30):s.value.history});},
  overview(ctx){const s=ctx.state();ctx.commit(s,{...s.value,screen:'overview'});},
  investigation(ctx){const s=ctx.state();ctx.commit(s,{...s.value,screen:'investigation'});},
  async investigate(ctx){let s=ctx.state();ctx.commit(s,{...s.value,busy:true,message:''});try{const result=await ctx.run('investigate');s=ctx.state();ctx.commit(s,{...s.value,message:result.admission==='already_running'?'Investigation already running':'Investigation requested'});}catch(e){s=ctx.state();ctx.commit(s,{...s.value,message:String(e)});}finally{s=ctx.state();ctx.commit(s,{...s.value,busy:false});}}
 }
});
