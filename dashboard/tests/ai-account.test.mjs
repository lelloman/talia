import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';

// Exercise the actual shell transport with a saved account error, then the UI.
let response={version:7,phase:'reconnect',error:'Account request failed; outcome may be unknown',origin:'https://ai.example',model:'code:smart'};
const context=vm.createContext({fetch:async()=>({ok:true,status:200,json:async()=>response}),location:{replace(){}}});
const auth=readFileSync(new URL('../web/auth.js',import.meta.url),'utf8');
vm.runInContext(auth.split('\n').find(line=>line.includes('const account=async'))+'\nglobalThis.account=account;',context);
assert.equal((await context.account({op:'aiAccountStatus'})).version,7);
response={error:'forbidden'};
await assert.rejects(context.account({op:'aiAccountBegin'}),e=>e.message==='forbidden'&&e.serverRejected);
response={version:7,phase:'reconnect',error:'saved failure'};
await assert.rejects(context.account({op:'users'}),/saved failure/);

const nodes=new Map();
const node=id=>{if(!nodes.has(id))nodes.set(id,{value:'',dataset:{},setAttribute(){},removeAttribute(){},querySelector(){return node('status-tone');}});return nodes.get(id);};
const requests=[];
const ui=vm.createContext({document:{getElementById:node},window:{addEventListener(){},dispatchEvent(){}},Event:class{},clearTimeout(){},setTimeout(){},location:{hash:'#settings'}});
vm.runInContext(readFileSync(new URL('../web/ai-account.js',import.meta.url),'utf8').replace('export async function','async function'),ui);
await ui.startAiAccount(async request=>{requests.push(request);return context.account(request);});
assert.equal(node('ai-account-status').textContent,'saved failure');
assert.equal(node('ai-account-connect').disabled,false);
await node('ai-account-connect').onclick();
assert.equal(requests.at(-1).op,'aiAccountBegin');
assert.equal(requests.at(-1).expected,7);
await ui.startAiAccount(async()=>{throw Error('offline');});
assert.equal(node('ai-account-connect').disabled,true);
assert.equal(node('ai-account-status').textContent,'offline');
console.log('AI account: saved errors remain actionable, rejections propagate, unavailable state disables actions');
