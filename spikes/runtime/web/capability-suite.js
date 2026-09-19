import {DashboardHost} from './dashboard-host.js';
const check = (ok, why) => { if (!ok) throw Error(why); };
export async function runCapabilitySuite(bridge) {
  const cases = await fetch('/shared/capabilities.json').then(r => r.json());
  const host = new DashboardHost(), checked = [];
  try {
    const survivor = await host.create(bridge, ['read']);
    for (const phase of ['saved','live']) for (const item of cases) {
      const guest = await host.create(bridge, ['read']);
      if (phase === 'live') await guest.eval("globalThis.engine={call(){}};globalThis.grants=['write'];globalThis.view={text:'changed'};void 0;");
      await guest.eval(`__send(${JSON.stringify(JSON.stringify(item.request))});void 0;`).catch(() => {});
      // A denied bridge request can race its command acknowledgement.
      for (let i=0; i<100 && guest.active; i++) await new Promise(r=>setTimeout(r,5));
      check(!guest.active, 'denied request did not retire guest');
      check(host.value===0, 'unauthorized effect');
      check(host.view(guest).text==='saved', 'view changed');
      check(await survivor.eval('1+1')===2, 'survivor failed');
      checked.push(`${phase}: ${item.name}`);
    }
    const victim = await host.create(bridge,['read']);
    host.request(victim,JSON.stringify({id:1,op:'write',value:99}));
    check(!victim.active && host.value===0,'dispatch bypass');
    const allowed = await host.create(bridge,['write','read']);
    await allowed.eval("(async()=>{await engine.write(8);assert(await engine.read('value')===8,'granted read/write');report.done=true;})();void 0;");
    for(let i=0;i<100 && !await allowed.eval('report.done');i++) await new Promise(r=>setTimeout(r,5));
    check(await allowed.eval('report.done') && host.value===8,'granted operations failed');
    return {passed:true,cases:checked,dispatch_recheck:true,granted_operations:true,view_unchanged:true,survivor_works:true};
  } finally {host.close();}
}
