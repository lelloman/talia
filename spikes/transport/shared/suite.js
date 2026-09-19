globalThis.report={done:false,checks:[]};
(async()=>{
  const a=new RemoteEngine('a'),b=new RemoteEngine('b');
  const check=(value,name)=>{if(!value)throw Error(name);report.checks.push(name);};
  const rejected=async(p,message)=>{try{await p;return false;}catch(e){return e.message===message;}};
  const until=async(predicate)=>{
    for(let i=0;i<500;i++){if(await predicate())return;await b.test('entered','pulse');}
    throw Error('condition timed out');
  };
  await Promise.all([a.connect(),b.connect()]);
  check(a.snapshot.value===0 && b.snapshot.value===0,'clients read initial server snapshot');
  await b.test('hold','slow-read');
  const stale=rejected(a.read('slow-read'),'stale snapshot');
  await until(()=>b.test('entered','slow-read'));
  await b.write('first',10).promise;
  check((await a.read()).value===10,'read and write progress while another read awaits network IO');
  await b.test('release','slow-read');
  check(await stale && a.snapshot.value===10,'late read cannot replace newer observed revision');

  const events=[];const unsubscribe=a.subscribe(s=>events.push(s));
  await b.write('second',20).promise;
  await until(()=>a.snapshot.value===20);
  check(events.length>=2 && events.every((v,i)=>i===0 || v.revision>events[i-1].revision),
    'subscription delivers ordered live changes from another client');
  unsubscribe();const eventCount=events.length;
  await b.write('third',21).promise;await a.read();
  check(events.length===eventCount,'unsubscribe stops delivery to listener');

  const skipped=a.write('skipped',99), skippedResult=rejected(skipped.promise,'cancelled');
  await skipped.cancel();
  check(await skippedResult && (await b.status('skipped')).status==='unknown',
    'cancelled work before dispatch never reaches server');
  await b.test('hold','cancel-before-effect');
  const cancelled=a.write('cancel-before-effect',99),cancelResult=rejected(cancelled.promise,'cancelled');
  await until(()=>b.test('entered','cancel-before-effect'));
  check((await cancelled.cancel()).status==='cancelled','server cancels accepted work before its effect');
  await b.test('release','cancel-before-effect');
  check(await cancelResult && (await b.read()).value===21,'cancelled accepted action produces no effect');
  const completed=a.write('cancel-after-effect',22);await completed.promise;
  check((await completed.cancel()).status==='completed' && (await b.read()).value===22,
    'cancellation after completion does not undo server effect');

  // Force identical IDs across generations; old network reply arrives while new ID is pending.
  a.disconnect();await a.connect();
  await b.test('hold','old-generation');
  const oldEpoch=a.epoch,oldId=a.next+1;let oldReplyArrived=false;
  const receive=a.receive.bind(a);
  a.receive=message=>{if(message.epoch===oldEpoch && message.id===oldId)oldReplyArrived=true;receive(message);};
  const old=rejected(a.read('old-generation'),'disconnected');
  await until(()=>b.test('entered','old-generation'));
  a.disconnect();check(await old,'disconnect rejects pending reads');
  await b.write('offline-change',30).promise;
  await a.connect();check(a.snapshot.value===30,'reconnect installs current snapshot after missed changes');
  await b.test('hold','new-generation');
  let settled=false;const replacementId=a.next+1;
  const replacement=a.read('new-generation').then(v=>{settled=true;return v;});
  await until(()=>b.test('entered','new-generation'));
  await b.test('release','old-generation');
  await until(()=>oldReplyArrived);
  check(oldId===replacementId && !settled && a.snapshot.value===30,'old generation reply cannot resolve colliding new request ID');
  await b.test('release','new-generation');
  check((await replacement).value===30,'replacement generation remains usable');

  await b.test('drop','lost-response');
  let unknown;
  try{await a.write('lost-response',40).promise;}catch(e){unknown=e;}
  check(unknown?.message==='outcome unknown' && unknown.actionId==='lost-response' && !a.connected,
    'lost HTTP response reports unknown action outcome with stable ID');
  await a.connect();
  const status=await a.status('lost-response'),revision=a.snapshot.revision;
  check(status.status==='completed' && status.result.value===40,'action status reconciles lost response after reconnect');
  await a.write('lost-response',40).promise;
  check(a.snapshot.revision===revision,'explicit retry with same action ID does not repeat effect');
  check(await rejected(a.write('lost-response',41).promise,'action ID conflict'),'reusing action ID with different arguments fails');

  await b.test('hold','accepted-offline');
  const offline=rejected(a.write('accepted-offline',50).promise,'outcome unknown');
  await until(()=>b.test('entered','accepted-offline'));a.disconnect();
  check(await offline && (await b.status('accepted-offline')).status==='accepted',
    'disconnect does not imply cancellation of accepted server work');
  await a.connect();await b.test('release','accepted-offline');
  await until(async()=>(await a.status('accepted-offline')).status==='completed');
  check((await a.read()).value===50,'accepted action completes independently of requester connection');
  await b.test('hold','older-completion-reply');
  const older=a.write('older-completion',60);
  await until(async()=>(await b.status('older-completion')).status==='completed');
  await b.write('newer-completion',70).promise;await a.read();
  await b.test('release','older-completion-reply');
  check((await older.promise).status==='completed' && a.snapshot.value===70,
    'older successful action response preserves newer displayed state');
  const resumed=[];const stop=a.subscribe(s=>resumed.push(s));
  a.disconnect();await b.write('while-offline',80).promise;await a.connect();
  check(a.snapshot.value===80 && resumed[resumed.length-1].value===80,
    'retained subscription receives reconnect snapshot');
  await b.write('after-reconnect',90).promise;await until(()=>a.snapshot.value===90);stop();
  check(resumed[resumed.length-1].value===90,'subscription resumes live delivery after reconnect');
  check((await a.status('missing')).status==='unknown','missing action record remains unknown rather than inferred failure');
  check(await rejected(a.rpc('write-anything',{}),'capability/arguments'),'server rejects unsupported capabilities');
  a.disconnect();b.disconnect();
  check(a.pending.size===0 && b.pending.size===0,'disconnect releases local pending operations');
  report.done=true;
})().catch(error=>{report.error=String(error)+'\n'+String(error.stack||'');report.done=true;});
