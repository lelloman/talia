#!/usr/bin/env python3
"""External MCP engine qualification: real stdio, engine, SQLite, and HTTP probe."""
import json,os,pathlib,subprocess,tempfile,time,sqlite3,sys
from mcp_authoring import MCP,BIN,provision,grant,put
from monitoring import EngineProcess,Fixture,wire,eventually

def main():
 fixture=Fixture();clients=[];checks=[];proxy=None
 with tempfile.TemporaryDirectory(prefix='talia-mcp-engine-') as temp:
  db=pathlib.Path(temp)/'engine.db';e=EngineProcess(db)
  engine=dict(family='engine',actions=['read','history','subscribe','write','set','run','run_status','cancel_run','resume_watch','audit'],scope={'kind':'all'})
  author=provision(db,temp,'agent',[grant(['list','read','validate','save','audit']),engine],ceiling=[dict(family='engine',actions=['read','write','run'],scope={'kind':'all'})])
  readonly=provision(db,temp,'reader',[dict(family='engine',actions=['read'],scope={'kind':'resource','id':'value'})])
  authoronly=provision(db,temp,'author-only',[grant(['list','read','validate','save'])])
  try:
   e.start();m=MCP(e.port,author);clients.append(m);reader=MCP(e.port,readonly);clients.append(reader);limited=MCP(e.port,authoronly);clients.append(limited)
   tools={t['name'] for t in m.rpc('tools/list',{})['result']['tools']};assert {'engine_read','engine_history','engine_subscribe','engine_poll','engine_unsubscribe','engine_write','engine_set','engine_run','engine_run_status','engine_cancel_run','engine_resume_watch'}<=tools
   def read(id='value'):return m.tool('engine_read',{'id':id})['sample']
   def write(value,request):return m.tool('engine_write',dict(id='value',expectedRevision=read()['revision'],value=value,requestId=request))
   def save(changes,request):return m.tool('definitions_save',{'requestId':request,'changeSet':{'expectedCatalogRevision':m.tool('definitions_list',{})['catalogRevision'],'changes':changes}})
   def instance(id):
    with sqlite3.connect(db) as s:return json.loads(s.execute('SELECT body FROM instances WHERE id=?',(id,)).fetchone()[0])
   def status(request):return m.tool('operation_status',{'requestId':request})['audit']
   changes=[]
   for id,source,initial in [
    ('setter',"{get(){return 0},async set(c,v){await c.write('value',7);c.state={committed:true,private:'PRIVATE_STATE'};await c.commit();await c.sleep(600);await c.write('value',99);return v}}",wire({})),
    ('counter',"{get(c){c.state++;return c.state}}",wire(0)),
    ('slow',"{async get(c){await c.sleep(200);return 0}}",wire({})),
    ('thrower',"{get(){throw Error('PRIVATE_EXCEPTION')}}",wire({}))]:
    changes.extend([put('variable_definition',id,dict(id=id,version=1,kind='computed',source=source,value_schema='any',state_schema='any',dependencies=['value'])),put('variable',id,dict(id=id,definition=id,params=wire({}),history_count=100,history_age_ms=60000),initial={'state':initial})])
   changes.extend([
    put('data_source','probe',dict(id='probe',kind='http',url=fixture.url,credential_ref='fixture')),
    put('monitor_definition','job',dict(id='job',version=1,kind='pipeline',source="{async run(c){await c.source('probe',{kind:'http',path:'/breakdown'});await c.sleep(400);return Infinity}}")),
    put('monitor_instance','job',dict(id='job',definition='job',sources={'probe':'probe'})),
    put('monitor_definition','watch',dict(id='watch',version=1,kind='watch',source="{async evaluate(c){const v=await c.read('v');if(v.value===42)throw Error('PRIVATE_EXCEPTION');c.state.ok=true}}")),
    put('monitor_instance','watch',dict(id='watch',definition='watch',inputs={'v':'value'}))
   ]);save(changes,'fixture')
   cases=[['undefined'],['number','NaN'],['number','Infinity'],['number','-Infinity'],['number','-0'],['object',[['x',['undefined']],['n',['number','NaN']]]]]
   for n,node in enumerate(cases):
    v={'version':1,'value':node};before=read();args=dict(id='value',expectedRevision=before['revision'],value=v,requestId=f'write-{n}')
    receipt=m.tool('engine_write',args);assert receipt['audit']['status']=='complete';assert m.tool('engine_write',args)['audit']['id']==receipt['audit']['id'];after=read();assert after['revision']==before['revision']+1 and after['value']==v;assert 'state' not in after and 'params' not in after
   seen=[];cursor=None
   while True:
    page=m.tool('engine_history',dict(id='value',limit=2,**({'cursor':cursor} if cursor else {})));seen+=page['samples'];cursor=page['nextCursor']
    if not cursor:break
   assert [s['value']['value'] for s in seen[:len(cases)]]==list(reversed(cases))
   stale=m.tool('engine_write',dict(id='value',expectedRevision=1,value=wire(2),requestId='stale'),False);assert stale['error']=='conflict' and stale['currentRevision']==read()['revision']
   checks.append('lossless exceptional values, bounded history, expected revisions and exact write deduplication')
   assert reader.tool('engine_read',{'id':'value'})['sample']['value']==read()['value']
   assert reader.tool('engine_history',{'id':'value'},False)['error']=='forbidden'
   assert limited.tool('engine_read',{'id':'value'},False)['error']=='forbidden'
   assert reader.tool('engine_write',dict(id='value',expectedRevision=read()['revision'],value=wire(2),requestId='deny'),False)['error']=='forbidden'
   assert 'error' in m.rpc('tools/call',{'name':'_session_close','arguments':{}})
   assert m.tool('engine_read',{'id':'value','principal':'agent'},False)['error']=='invalid_input'
   assert 'PRIVATE_EXCEPTION' not in json.dumps(m.tool('engine_read',{'id':'thrower'},False))
   assert m.tool('engine_read',{'id':'slow','timeoutMs':1},False)['error']=='timed_out'
   checks.append('separate permission families, bounded async reads and sanitized failures without private Variable state')
   sub=m.tool('engine_subscribe',{'ids':['value']});assert sub['values'][0]['value']==read()['value']
   other=MCP(e.port,author);clients.append(other);assert other.tool('engine_poll',{'subscriptionId':sub['subscriptionId']},False)['error']=='not_found'
   assert not m.tool('engine_poll',{'subscriptionId':sub['subscriptionId']})['values'];write(wire(5),'stream-update');assert m.tool('engine_poll',{'subscriptionId':sub['subscriptionId']})['values'][0]['value']==wire(5)
   m.tool('engine_unsubscribe',{'subscriptionId':sub['subscriptionId']});assert m.tool('engine_poll',{'subscriptionId':sub['subscriptionId']},False)['error']=='not_found'
   lease=other.tool('engine_subscribe',{'ids':['counter']});eventually(lambda:instance('counter')['has_value']);other.close();clients.remove(other);time.sleep(.1);before=instance('counter')['revision'];write(wire(6),'after-close');time.sleep(.15);assert instance('counter')['revision']==before
   checks.append('connection-owned coalesced subscriptions, explicit release and cleanup on MCP EOF')
   # A pending setter yields, so another connection can operate while it waits.
   expected=read('setter')['revision'];m.seq+=1;pending=m.seq;m.send({'jsonrpc':'2.0','id':pending,'method':'tools/call','params':{'name':'engine_set','arguments':dict(id='setter',expectedRevision=expected,value=wire(123),requestId='cancel-set')}})
   eventually(lambda:instance('value')['value']==wire(7))
   assert reader.tool('engine_read',{'id':'value'})['sample']['value']==wire(7)
   m.send({'jsonrpc':'2.0','method':'notifications/cancelled','params':{'requestId':pending,'reason':'qualification'}})
   eventually(lambda:status('cancel-set')['status'] in ['cancelled','failed'])
   assert status('cancel-set')['status']=='cancelled';assert instance('value')['value']==wire(7);assert instance('setter')['state']==wire({'committed':True,'private':'PRIVATE_STATE'})
   checks.append('async setter yields; MCP cancellation preserves earlier commits and fences subsequent effects')
   admitted=m.tool('engine_run',dict(id='job',requestId='run-once'));run=admitted['admission']['run_id'];eventually(lambda:fixture.probes==1)
   assert m.tool('engine_run',dict(id='job',requestId='run-once'))==admitted
   assert m.tool('operation_status',{'requestId':'run-once'})['admission']==admitted['admission']
   assert reader.tool('engine_run_status',{'runId':run},False)['error']=='forbidden'
   eventually(lambda:m.tool('engine_run_status',{'runId':run})['run']['status']=='complete');assert fixture.probes==1;assert m.tool('engine_run_status',{'runId':run})['run']['result']=={'version':1,'value':['number','Infinity']}
   cancel=m.tool('engine_run',dict(id='job',requestId='run-cancel'))['admission']['run_id'];eventually(lambda:fixture.probes==2);m.tool('engine_cancel_run',dict(runId=cancel,requestId='cancel-run'));assert m.tool('engine_run_status',{'runId':cancel})['run']['status']=='cancelled'
   checks.append('durable Pipeline admission, run ownership, tagged results and authorized cancellation without replay')
   write(wire(42),'fault-watch')
   def monitoring():return m.tool('engine_read',{'id':'monitor.watch'})['sample']
   eventually(lambda:monitoring()['evaluation']=='error');assert 'PRIVATE_EXCEPTION' not in json.dumps(monitoring());write(wire(43),'repair-watch');m.tool('engine_resume_watch',dict(id='watch',requestId='resume'));eventually(lambda:monitoring()['evaluation']=='idle')
   checks.append('monitoring resource access and explicit Watch resume with sanitized exceptions')
   if '--clients' in sys.argv:
    write({'version':1,'value':['number','Infinity']},'for-clients')
    proxy=subprocess.Popen(['python3','dashboard/serve.py','0'],env={**os.environ,'TALIA_ENGINE_DB':str(db),'TALIA_ENGINE_PORT':str(e.port)},stdout=subprocess.PIPE,text=True)
    port=json.loads(proxy.stdout.readline())['port']
    subprocess.run(['node','engine/tests/mcp-engine-web.mjs',str(port)],check=True)
    subprocess.run(['python3','engine/tests/mcp_engine_android.py',os.environ.get('TALIA_EMULATOR','emulator-5570'),str(port)],check=True)
    proxy.terminate();proxy.wait(timeout=10);proxy=None
    checks.append('MCP-written exceptional value consumed by web and native emulator dashboards')
   # Reconnect and server restart preserve audit identities and run admission, never old leases.
   active=m.tool('engine_subscribe',{'ids':['value']});m.close();clients.remove(m);m=MCP(e.port,author);clients.append(m);assert m.tool('engine_poll',{'subscriptionId':active['subscriptionId']},False)['error']=='not_found'
   assert m.tool('engine_run',dict(id='job',requestId='run-once'))==admitted;assert fixture.probes==2
   write(wire(0),'before-crash')
   crash_args=dict(id='setter',expectedRevision=read('setter')['revision'],value=wire(321),requestId='crash-set')
   m.seq+=1;m.send({'jsonrpc':'2.0','id':m.seq,'method':'tools/call','params':{'name':'engine_set','arguments':crash_args}})
   eventually(lambda:instance('value')['value']==wire(7))
   e.stop(kill=True)
   for c in clients:c.close()
   clients=[];e.start();m=MCP(e.port,author);clients.append(m);
   assert status('crash-set')['status']=='unknown'
   assert m.tool('engine_set',crash_args,False)['audit']['status']=='unknown'
   assert instance('value')['value']==wire(7)
   assert m.tool('operation_status',{'requestId':'run-once'})['admission']==admitted['admission'];assert m.tool('engine_run',dict(id='job',requestId='run-once'))==admitted;assert fixture.probes==2
   audit=m.tool('audit_list',{'limit':100});text=json.dumps(audit);assert all(v not in text for v in ['PRIVATE_STATE','PRIVATE_EXCEPTION','fixture-secret',author.read_text()])
   checks.append('reconnect/restart outcome reconciliation and sanitized durable audit records')
   print(json.dumps({'passed':True,'checks':checks}))
  finally:
   for c in clients:c.close()
   if proxy:proxy.terminate();proxy.wait(timeout=10)
   e.stop();fixture.close()
if __name__=='__main__':main()
