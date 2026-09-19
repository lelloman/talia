import '../host/execution.js';
import '../host/execution-suite.js';
import {DashboardHost} from './dashboard-host.js';
export async function runExecutionBoundarySuite(bridge) {
  const checks=testAuthority(), host=new DashboardHost(), authority=new ExecutionAuthority();
  authority.define('x','independent',0);
  try {
    for(const stale of [false,true]) {
      const task=authority.begin('x'), guest=await host.create(bridge,['write']);
      host.bindExecution(guest,authority,task.task);
      if(stale)authority.invalidate('x');else authority.cancel(task.lease);
      await guest.eval(`__send('{"id":1,"op":"write","value":99}');void 0;`).catch(()=>{});
      for(let i=0;i<100 && guest.active;i++)await new Promise(r=>setTimeout(r,5));
      if(guest.active || host.value!==0 || !guest.reason.includes(stale?'stale':'cancelled'))throw Error('execution guard bypass');
      authority.settle(task.task,99);
      if(!authority.outcome(task.lease).error)throw Error('obsolete result published');
    }
    const task=authority.begin('x','write'), guest=await host.create(bridge,['write','state.read','state.commit']);
    host.bindExecution(guest,authority,task.task);
    await guest.eval("(async()=>{await engine.call('state.commit',7);assert(await engine.call('state.read')===7,'protected state');await engine.write(8);report.done=true;})();void 0;");
    for(let i=0;i<100 && !await guest.eval('report.done');i++)await new Promise(r=>setTimeout(r,5));
    if(!await guest.eval('report.done'))throw Error('protected state did not finish');
    authority.cancel(task.lease);
    if(host.value!==8 || authority.snapshot(authority.begin('x').task)!==7)throw Error('prior effects undone');
    return {passed:true,checks,raw_cancelled_effect_rejected:true,raw_stale_effect_rejected:true,protected_state:true,prior_effect_survives:true};
  } finally {host.close();}
}
