{async evaluate(ctx){
 const sample=await ctx.read('host'),now=ctx.now(),g=sample.value?.gitWorkspaces;
 const validTime=Number.isFinite(g?.checked)&&g.checked>0&&g.checked*1000<=now+30000;
 const deadline=(validTime?g.checked*1000:ctx.params.enabledAt)+86700000;
 const overdue=now>deadline;
 const fresh=sample.hasValue&&sample.quality==='good'&&now-sample.timestamp<=90000;
 const stage=()=>ctx.state.stage||'daily-a';
 const result=(active,message,currentStage=stage())=>({active,stage:currentStage,severity:'warning',message});
 const name=ctx.params.host;
 let token,message;
 if(overdue){
  token='overdue-'+Math.floor((now-deadline)/86400000);
  message=name+': daily Git workspace check is overdue or unavailable. Last check: '+(validTime?new Date(g.checked*1000).toISOString():'none recorded')+'.';
 }else{
  // Hold existing alert state while awaiting fresh data; do not turn unknown into clean.
  if(!fresh||!validTime)return result(!!ctx.alert?.active,ctx.alert?.message||'Waiting for the next daily Git check.',ctx.alert?.stage||'quiet');
  const dirty=g.dirty||[],errors=g.errors||[];
  const clean=g.success===1&&g.repositories>0&&g.worktrees>0&&!g.dataWarning&&!dirty.length&&!errors.length;
  if(clean)return result(false,name+': all configured Git worktrees are clean.');
  if(g.automatic!==1)return result(true,name+': manual Git check needs attention; see the dashboard.','quiet');
  token='check-'+g.checked;
  const paths=list=>list.slice(0,8).map(p=>p.replace(/^\/home\/[^/]+\//,'~/')).join(', ')+(list.length>8?' … +'+(list.length-8)+' more':'');
  const problems=[];
  if(dirty.length)problems.push('Dirty worktrees: '+paths(dirty));
  if(errors.length)problems.push('Could not check: '+paths(errors));
  if(g.success!==1||!g.repositories||!g.worktrees||g.dataWarning)problems.push('Repository check incomplete');
  message=name+': daily Git workspace check needs attention.\n'+problems.join('\n')+'\nChecked: '+new Date(g.checked*1000).toISOString();
 }
 if(ctx.state.token!==token){
  ctx.state.token=token;
  // A new stage starts one durable delivery, even when yesterday also warned.
  ctx.state.stage=ctx.alert?.stage==='daily-a'?'daily-b':'daily-a';
 }
 return result(true,message);
}}
