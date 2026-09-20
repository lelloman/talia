import test from 'node:test';import assert from 'node:assert/strict';import vm from 'node:vm';import {readFileSync} from 'node:fs';
const library=readFileSync(new URL('../shared/vm.js',import.meta.url),'utf8');
function guest(source){const requests=[],c=vm.createContext({__send:s=>requests.push(JSON.parse(s))});vm.runInContext(library+source+';TaliaVM.start({});',c);return {requests,run:s=>vm.runInContext(s,c),state:()=>JSON.parse(vm.runInContext('JSON.stringify(TaliaVM.snapshot())',c))};}
const flush=async()=>{for(let i=0;i<20;i++)await Promise.resolve();};
test('async actions interleave; stale commit fails without undoing earlier effects',async()=>{
 const g=guest(`defineVM({initial:()=>({n:0}),actions:{async slow(c){let s=c.state();await c.read('value');c.commit(s,{n:1});},fast(c){let s=c.state();c.commit(s,{n:2});}}});`);
 g.run(`TaliaVM.dispatch('slow',{target:'x',value:null})`);await flush();g.run(`TaliaVM.dispatch('fast',{target:'x',value:null})`);await flush();
 assert.equal(g.state().state.n,2);g.run(`TaliaVM.receive('${JSON.stringify({id:1,value:10})}')`);await flush();assert.match(g.state().failure,/stale snapshot/);assert.equal(g.state().state.n,2);
});
test('pause cancels pending work, forbids late effects and keeps retained state',async()=>{
 const g=guest(`defineVM({initial:()=>({n:1}),actions:{async work(c){try{await c.read('x')}catch{} await c.write(2);}}});`);
 g.run(`TaliaVM.dispatch('work',{target:'x',value:null})`);await flush();g.run('TaliaVM.markDirty();TaliaVM.pause();TaliaVM.resume();');await flush();
 assert.equal(g.requests.length,1);assert.equal(g.state().dirty,true);assert.equal(g.state().failure,null);
});
test('VM references have independent state; functions resolve by name',async()=>{
 const g=guest(`defineFunction('double',x=>x*2);defineVMReference('counter',{initial:p=>({n:p.n}),actions:{inc(c){let s=c.state();c.commit(s,{n:s.value.n+1})}}});defineVM({initial:()=>({a:0,b:0}),actions:{async go(c){const a=c.instance('a','counter',{n:1}),b=c.instance('b','counter',{n:8});await a.dispatch('inc');let s=c.state();c.commit(s,{a:c.fn('double',a.state().n),b:b.state().n})}}});`);
 g.run(`TaliaVM.dispatch('go',{target:'x',value:null})`);await flush();assert.deepEqual(g.state().state,{a:4,b:8});
});
test('shared example handles engine subscriptions and control actions',async()=>{
 const g=guest(readFileSync(new URL('../examples/monitor.vm.js',import.meta.url),'utf8'));await flush();assert.equal(g.requests[0].op,'subscribe');
 g.run(`TaliaVM.receive('${JSON.stringify({id:1,value:'s1'})}')`);await flush();g.run(`TaliaVM.receive('${JSON.stringify({event:'s1',value:{value:4,revision:1}})}')`);await flush();
 g.run(`TaliaVM.dispatch('controls',{target:'controls',value:null})`);await flush();assert.equal(g.state().state.screen,'controlsScreen');assert.deepEqual(g.state().state.history,[4]);assert.equal(g.state().subscriptions.length,1);
});

test('referenced ViewModels receive start and resume lifecycle independently',async()=>{
 const g=guest(`defineVMReference('child',{initial:p=>({n:p.n}),start(c){const s=c.state();c.commit(s,{n:s.value.n+10})},resume(c){const s=c.state();c.commit(s,{n:s.value.n+100})},actions:{read(c){return c.state().value}}});defineVM({initial:()=>({a:null,b:null}),actions:{async inspect(c){const a=c.instance('a','child',{n:1}),b=c.instance('b','child',{n:2});await a.dispatch('read');const s=c.state();c.commit(s,{a:a.state(),b:b.state()});}}});`);
 g.run(`TaliaVM.dispatch('inspect',{target:'x',value:null})`);await flush();assert.deepEqual(g.state().state,{a:{n:11},b:{n:12}});
 g.run('TaliaVM.pause();TaliaVM.resume();TaliaVM.reconcile([]);');await flush();g.run(`TaliaVM.dispatch('inspect',{target:'x',value:null})`);await flush();assert.deepEqual(g.state().state,{a:{n:111},b:{n:112}});
});
