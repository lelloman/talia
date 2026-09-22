// Service worker contract: no vendor account or desktop notifications required.
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const handlers={},shown=[],requests=[],opened=[];
let result={ok:true,json:async()=>({alert:{key:'disk',active:true,severity:'critical',message:'Disk almost full'}})},networkError=false;
const context=vm.createContext({Date,Number,URL,AbortSignal,fetch:async(url,options)=>{requests.push({url,options});if(networkError)throw Error('offline');return result;},self:{location:{origin:'https://talia.example'},addEventListener:(type,fn)=>handlers[type]=fn,registration:{showNotification:async(title,options)=>shown.push({title,options})},clients:{matchAll:async()=>[],openWindow:async url=>opened.push(url)}}});
vm.runInContext(readFileSync(new URL('../web/push-sw.js',import.meta.url),'utf8'),context);
const payload={version:1,device:'browser-fixture',key:'disk',occurrence:1,revision:3,expires:Date.now()+60000};
async function push(p=payload){let promise;handlers.push({data:{json:()=>p},waitUntil:p=>promise=p});await promise;}
await push();assert.equal(shown.length,1);assert.equal(shown[0].options.body,'Disk almost full');assert.equal(requests[0].options.credentials,'same-origin');assert.equal(JSON.parse(requests[0].options.body).revision,3);
for(const invalid of [{...payload,expires:0},{...payload,revision:'3'},{...payload,version:2},null])await push(invalid);
assert.equal(shown.length,1);assert.equal(requests.length,1);
result={ok:false};await push();assert.equal(shown.length,1);
result={ok:true,json:async()=>({error:'notification superseded'})};await push();assert.equal(shown.length,1);
networkError=true;await push();assert.equal(shown.length,2);assert.equal(shown[1].title,'Talìa');assert.equal(shown[1].options.body.includes('Disk'),false);
let click;handlers.notificationclick({notification:{close(){},data:{url:'https://attacker.example'}},waitUntil:p=>click=p});await click;assert.equal(opened[0],'https://talia.example/#alerts');
console.log('Browser push worker: authenticated details, stale/denied suppression, generic offline notice, safe click passed');
