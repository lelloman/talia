import {test} from 'node:test';
import assert from 'node:assert/strict';
import {DashboardPlaylist,normalizePlaylist} from '../web/monitoring-mode.js';
const entries=[{id:'first',seconds:5},{id:'second',seconds:10}];
test('settings validate untrusted stored values, bound size and retain order',()=>{
 assert.deepEqual(normalizePlaylist({auto:true,entries:[null,{seconds:5},...entries,entries[0],{id:'bad id',seconds:5},{id:'short',seconds:0},{id:'float',seconds:5.1}]}),{version:1,auto:true,entries});
 assert.equal(normalizePlaylist({entries:Array.from({length:100},(_,i)=>({id:'D'+i,seconds:5}))}).entries.length,32);
});
function fixture(){let time=0,current='first',reason='',select=async id=>{current=id;};const calls=[],messages=[];const p=new DashboardPlaylist({now:()=>time,current:()=>current,select:async id=>{calls.push(id);await select(id);},blocked:()=>reason,notify:m=>messages.push(m)});p.configure(entries);p.active=p.playing=true;return {p,calls,messages,advance:n=>time+=n,block:v=>reason=v,select:f=>select=f,current:()=>current};}
test('per-entry dwell, manual wrap, pause and no background catch-up',async()=>{
 const f=fixture();f.p.tick();f.advance(4999);f.p.tick();assert.equal(f.calls.length,0);f.advance(1);f.p.tick();await new Promise(setImmediate);assert.equal(f.current(),'second');
 f.p.tick();f.advance(9999);f.p.tick();assert.equal(f.calls.length,1);
 f.p.tick(true);f.advance(60000);f.p.tick();assert.equal(f.calls.length,1);f.advance(10000);f.p.tick();await new Promise(setImmediate);assert.equal(f.current(),'first');
 f.p.pause();f.advance(60000);f.p.tick();assert.equal(f.calls.length,2);await f.p.step(-1);assert.equal(f.current(),'second');
});
test('slow switches serialize and dwell starts only after completion',async()=>{
 const f=fixture();let release;f.select(()=>new Promise(r=>release=r));f.p.tick();f.advance(5000);f.p.tick();await f.p.step(1);f.advance(90000);f.p.tick();assert.equal(f.calls.length,1);assert.equal(f.p.busy,true);release();await new Promise(setImmediate);assert.equal(f.p.deadline,null);
});
test('dirty/error block rotation; rejected switch pauses without retry storm',async()=>{
 const f=fixture();f.block('dirty');f.p.tick();assert.equal(f.p.playing,false);assert.deepEqual(f.messages,['dirty']);assert.equal(f.calls.length,0);
 f.block('');f.p.playing=true;f.select(async()=>{throw Error('denied');});await f.p.step();assert.equal(f.p.playing,false);assert.equal(f.messages.at(-1),'Rotation paused: denied');f.advance(60000);f.p.tick();assert.equal(f.calls.length,1);
});
