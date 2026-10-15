import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const rules=JSON.parse(readFileSync(new URL('rules.json',import.meta.url)));
const policy=Function('return ('+readFileSync(new URL('condition.js',import.meta.url),'utf8')+')')();
for(const name of ['ServiceDown','PeppinoHighErrorRate','PeppinoReadinessFailing']) {
 const rule=rules.find(r=>r.name===name);
 assert.ok(rule,name);
 let now=100000;
 const sample={hasValue:true,quality:'good',timestamp:now,value:{checked:now,series:[]}};
 const ctx={state:{},params:{...rule.annotations,name,forMs:rule.forMs,staleMs:rule.intervalMs*3,severity:rule.labels.severity},now:()=>now,read:async()=>sample};
 const series=[{labels:{job:'peppino',service:'peppino-ready'},value:0}];
 for(let elapsed=0;elapsed<=rule.forMs;elapsed+=rule.intervalMs) {
   sample.timestamp=now;sample.value={checked:now,series};
   assert.equal((await policy.evaluate(ctx)).active,elapsed>=rule.forMs,name+' duration');
   now+=rule.intervalMs;
 }
 sample.quality='error';
 await assert.rejects(policy.evaluate(ctx),'failed query cannot resolve '+name);
 sample.quality='good';sample.timestamp=now;sample.value={checked:now,series:[]};
 assert.equal((await policy.evaluate(ctx)).active,false,name+' recovery');
}
console.log('Peppino policy timing, failure and recovery passed.');
