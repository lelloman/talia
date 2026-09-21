#!/usr/bin/env python3
"""External newline-JSON MCP client, real adapter/service, temporary DB and HTTP probes.
Optional --clients exercises the authored package in Chromium and the emulator.
"""
import copy,json,os,pathlib,queue,secrets,sqlite3,subprocess,sys,tempfile,threading,urllib.request
import monitoring
from monitoring import Fixture,EngineProcess,config,wire,eventually,ROOT
BIN=pathlib.Path(os.environ.get('TALIA_BIN_DIR','/tmp/talia-p3-target/debug'))
monitoring.BIN=str(BIN/'talia-engine')
class MCP:
 def __init__(self,port,credential,version='2025-11-25'):
  self.proc=subprocess.Popen([str(BIN/'talia-mcp'),f'http://127.0.0.1:{port}',str(credential)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
  self.q=queue.Queue();self.seq=0
  def read():
   for line in self.proc.stdout:self.q.put(json.loads(line))
   self.q.put(None)
  self.thread=threading.Thread(target=read,daemon=True);self.thread.start()
  response=self.rpc('initialize',{'protocolVersion':version,'capabilities':{},'clientInfo':{'name':'external-authoring-qualification','version':'1'}})
  assert response['result']['protocolVersion']==version,response
  self.send({'jsonrpc':'2.0','method':'notifications/initialized'})
 def send(self,body):self.proc.stdin.write(json.dumps(body)+'\n');self.proc.stdin.flush()
 def rpc(self,method,params):
  self.seq+=1;self.send({'jsonrpc':'2.0','id':self.seq,'method':method,'params':params})
  while True:
   response=self.q.get(timeout=40)
   assert response is not None,'adapter exited'
   if response.get('id')==self.seq:return response
 def tool(self,name,args,ok=True):
  response=self.rpc('tools/call',{'name':name,'arguments':args});assert 'error' not in response,response
  r=response['result'];v=r['structuredContent'];assert json.loads(r['content'][0]['text'])==v
  if ok:assert not r['isError'],v
  else:assert r['isError'],v
  return v
 def close(self):
  self.proc.stdin.close();self.proc.wait(timeout=10);self.thread.join(timeout=2)
  assert self.proc.returncode==0,self.proc.stderr.read()
def put(kind,id,document,**kwargs):return dict(op='put',key=dict(kind=kind,id=id),document=document,**kwargs)
def grant(actions,scope=None):return dict(family='authoring',actions=actions,scope=scope or {'kind':'all'})
def provision(db,temp,principal,grants,expected=0,enabled=True,ceiling=None):
 policy=pathlib.Path(temp)/(principal+'.json');token=pathlib.Path(temp)/(principal+'.token')
 policy.write_text(json.dumps(dict(principal=principal,expectedVersion=expected,enabled=enabled,grants=grants,ceiling=ceiling)))
 subprocess.run([str(BIN/'talia-agent'),str(db),str(policy),str(token) if expected==0 else '--policy-only'],check=True,capture_output=True)
 if expected==0:assert token.stat().st_mode&0o777==0o600
 return token
def main():
 checks=[];fixture=Fixture()
 with tempfile.TemporaryDirectory(prefix='talia-mcp-') as temp:
  db=pathlib.Path(temp)/'engine.db';e=EngineProcess(db);clients=[];proxy=None
  full=[grant(['read','list','validate','save','assign','audit'])]
  credential=provision(db,temp,'author',full,ceiling=[dict(family='engine',actions=['read','write','run'],scope={'kind':'all'})])
  limited=provision(db,temp,'limited',[grant(['list'],{'kind':'definition','definition_kind':'ui','id':'notice'})])
  try:
   e.start();m=MCP(e.port,credential);clients.append(m)
   names={t['name'] for t in m.rpc('tools/list',{})['result']['tools']};assert names>={'definitions_list','definitions_read','definitions_validate','definitions_save','client_assignment_set','operation_status','audit_list'}
   modern=MCP(e.port,credential,'2026-07-28');modern.tool('definitions_list',{});modern.close()
   checks.append('MCP initialization, both protocol generations and tool discovery')
   def revision():return m.tool('definitions_list',{})['catalogRevision']
   def bundle(changes):return dict(expectedCatalogRevision=revision(),changes=changes)
   def save(id,b,ok=True):return m.tool('definitions_save',{'requestId':id,'changeSet':b},ok)
   fragment='<Column id="Counter"><Text id="Count" text={state.n}/><Button id="Add" text="Increment" onClick={actions.increment}/></Column>'
   dashboard={'ui':'<Dashboard id="Authored"><Surface id="Body"><Use id="Main" definition="counter" params={params.counter}/></Surface></Dashboard>','view_model':'defineVM({initial:()=>({n:0}),actions:{async increment(ctx){const child=ctx.instance("counter","counterVm");await child.dispatch("increment");const s=ctx.state();ctx.commit(s,child.state());}}});','references':[{'kind':'ui','id':'counter'},{'kind':'vm','id':'counterVm'}],'params':{'counter':{}},'grants':{'reads':[],'writes':[],'runs':[]}}
   changes=[put('ui','counter',{'source':fragment}),put('function','addTwo',{'source':'n=>n+2'}),put('vm','counterVm',{'source':'{initial:()=>({n:0}),actions:{increment(ctx){const s=ctx.state();ctx.commit(s,{n:ctx.fn("addTwo",s.value.n)});}}}','references':[{'kind':'function','id':'addTwo'}]}),put('dashboard','authored',dashboard),put('dashboard','authoredTwo',dashboard)]
   c=config(fixture.url)
   changes.append(put('variable_definition','measurements',dict(id='measurements',version=1,kind='stored',source='',value_schema='any',state_schema='any',dependencies=[])))
   for id in ['cpu','memory','disk','disk-y','breakdown']:changes.append(put('variable',id,dict(id=id,definition='measurements',params=wire({}),history_count=20,history_age_ms=60000),initial={'state':wire({})}))
   for kind,items in [('data_source',c['sources']),('monitor_definition',c['definitions']),('monitor_instance',c['instances'])]:
    changes.extend(put(kind,d['id'],d) for d in items)
   b=bundle(changes);assert m.tool('definitions_validate',{'changeSet':b})['valid'];assert revision()==b['expectedCatalogRevision'];assert fixture.calls==0
   saved=save('create',b);assert saved['audit']['status']=='complete';rev=revision();assert rev>b['expectedCatalogRevision']
   assert save('create',b)['audit']['id']==saved['audit']['id'];assert revision()==rev
   assert save('conflict',b,False)['error']=='conflict'
   assert save('create',bundle([put('ui','counter',{'source':fragment})]),False)['error']=='conflict'
   eventually(lambda:fixture.calls>=8);assert e.ok('read',{'id':'cpu'})['value']==wire(42)
   checks.append('atomic shared UI/VM/function and automatic Prometheus/watch authoring without restart')
   checks.append('validation rollback, durable deduplication and revision/request conflicts')
   bad=bundle([put('ui','broken',{'source':'<Text id="Bad" text="broken">'}),put('function','uncommitted',{'source':'n=>n'})])
   validation=m.tool('definitions_validate',{'changeSet':bad},False);assert validation['diagnostics'][0]['line']>=1
   failure=save('bad-source',bad,False);assert failure['diagnostics'][0]['message']==validation['diagnostics'][0]['message'];assert revision()==rev
   denied=MCP(e.port,limited);clients.append(denied)
   page=denied.tool('definitions_list',{});assert len(page['records'])==1 and page['records'][0]['key']['id']=='notice'
   assert denied.tool('definitions_read',{'keys':[{'kind':'ui','id':'notice'}]},False)['error']=='forbidden'
   denial=denied.tool('definitions_save',{'requestId':'denied','changeSet':b},False);assert denial['error']=='forbidden'
   assert denied.tool('audit_list',{},False)['error']=='forbidden'
   assert m.tool('definitions_list',{'principal':'author'},False)['error']=='invalid_input'
   first=m.tool('definitions_list',{'limit':1});cursor=first['nextCursor'];assert m.tool('definitions_list',{'cursor':cursor,'limit':1})['records']!=first['records']
   assert m.tool('definitions_list',{'limit':0},False)['error']=='invalid_input'
   assert 'error' in m.rpc('tools/call',{'name':'live_execute','arguments':{}})
   for token,origin,code in [('',None,'unauthenticated'),(credential.read_text(),'http://evil.invalid','forbidden')]:
    headers={'Content-Type':'application/json','Authorization':'Bearer '+token}
    if origin:headers['Origin']=origin
    req=urllib.request.Request(f'http://127.0.0.1:{e.port}/agent',data=b'{"name":"definitions_list"}',headers=headers)
    with urllib.request.urlopen(req,timeout=5) as response:assert json.load(response)['error']==code
   checks.append('located source diagnostics, rollback, strict arguments and scoped authorization')
   # Registered clients use their host credentials; MCP assignment does not reload them.
   host=secrets.token_hex(32);owner=secrets.token_hex(32)
   def client(body):
    request=urllib.request.Request(f'http://127.0.0.1:{e.port}/clients',data=json.dumps(body).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+host})
    with urllib.request.urlopen(request,timeout=5) as r:response=json.load(r)
    assert 'error' not in response,response
    return response['value']
   registration=client({'op':'register','name':'MCP fixture','platform':'web'});assigned=client({'op':'openSlot','slot':'fixture','owner':owner})
   args={'clientId':registration['clientId'],'slotId':'fixture','expectedRevision':assigned['revision'],'assignment':{'dashboardId':'authored','params':{'label':'MCP'},'presentation':{'scale':1.5}},'requestId':'assign'}
   assignment=m.tool('client_assignment_set',args);assert assignment['assignment']['dashboardId']=='authored'
   assert m.tool('client_assignment_set',args)['audit']['id']==assignment['audit']['id']
   stale=copy.deepcopy(args);stale['requestId']='stale-assignment';assert m.tool('client_assignment_set',stale,False)['error']=='conflict'
   delivery=client({'op':'delivery','slot':'fixture','owner':owner});old_package=delivery['package'];assert old_package['params']['label']=='MCP'
   if '--clients' in sys.argv:
    proxy=subprocess.Popen(['python3','dashboard/serve.py','0'],env={**os.environ,'TALIA_ENGINE_DB':str(db),'TALIA_ENGINE_PORT':str(e.port)},stdout=subprocess.PIPE,text=True)
    port=json.loads(proxy.stdout.readline())['port']
    subprocess.run(['node','dashboard/tests/mcp-authoring.mjs',str(port)],check=True)
    serial=os.environ.get('TALIA_EMULATOR','emulator-5570')
    subprocess.run(['python3','dashboard/tests/mcp_authoring_android.py',serial,str(port)],check=True)
    checks.append('MCP-authored interactive shared package on web and native emulator')
   edited=save('shared-update',bundle([put('function','addTwo',{'source':'n=>n+3'})]));assert len(edited['receipt']['packages'])==2
   newer=client({'op':'delivery','slot':'fixture','owner':owner})['package'];assert newer['revision']!=old_package['revision'];assert 'n=>n+2' in old_package['viewModel'] and 'n=>n+3' in newer['viewModel']
   assert m.tool('definitions_list',{'cursor':cursor},False)['error']=='conflict'
   checks.append('revisioned desired assignments and coherent shared package updates')
   # A migration exception must not expose the runtime state it can inspect.
   secret='private-migration-state';d=m.tool('definitions_read',{'keys':[{'kind':'variable_definition','id':'stored'}]})['records'][0]['document'];d['version']+=1;d['source']=' '
   bad_migration=bundle([put('variable_definition','stored',d,migration=f'()=>{{throw Error("{secret}")}}')])
   diagnostic=m.tool('definitions_validate',{'changeSet':bad_migration},False);assert secret not in json.dumps(diagnostic);assert diagnostic['diagnostics'][0]['path']=='migration',diagnostic
   assert secret not in json.dumps(save('bad-migration',bad_migration,False))
   all_audits=[];cur=None
   while True:
    page=m.tool('audit_list',dict(limit=2,**({'cursor':cur} if cur else {})));all_audits+=page['records'];cur=page['nextCursor']
    if not cur:break
   audit_text=json.dumps(all_audits);assert secret not in audit_text and credential.read_text() not in audit_text and 'n=>n+3' not in audit_text
   assert any(a['request_id']=='denied' and a['status']=='failed' for a in all_audits),all_audits
   assert m.tool('operation_status',{'requestId':'create'})['audit']['id']==saved['audit']['id']
   checks.append('permission denials and sanitized audits/status across all mutations')
   deletion=bundle([dict(op='delete',key={'kind':'function','id':'addTwo'})]);assert save('referenced-delete',deletion,False)['error']=='validation_failed'
   delete=bundle([dict(op='delete',key={'kind':kind,'id':id}) for kind,id in [('dashboard','authored'),('dashboard','authoredTwo'),('ui','counter'),('vm','counterVm'),('function','addTwo')]])
   save('delete-bundle',delete);assert m.tool('definitions_read',{'keys':[{'kind':'dashboard','id':'authored'}]},False)['error']=='not_found'
   checks.append('referenced deletion rejection and atomic closure deletion')
   computed=dict(id='computed',version=1,kind='computed',source='{get(c){return c.params.n}}',value_schema='any',state_schema='any',dependencies=[])
   variable=dict(id='computed',definition='computed',params=wire({'n':10}),history_count=10,history_age_ms=60000)
   save('computed-create',bundle([put('variable_definition','computed',computed),put('variable','computed',variable)]))
   e.ok('subscribe',{'id':'computed'})
   def computed_value():return next(v['value'] for v in e.ok('snapshot')['values'] if v['id']=='computed')
   eventually(lambda:computed_value()==wire(10))
   variable['params']=wire({'n':20});save('computed-params',bundle([put('variable','computed',variable)]))
   eventually(lambda:computed_value()==wire(20))
   checks.append('hot computed parameter activation refreshes subscribers without an explicit read')

   for a in clients:a.close()
   clients=[];e.stop();e.start();m=MCP(e.port,credential);clients.append(m)
   assert m.tool('operation_status',{'requestId':'create'})['audit']['id']==saved['audit']['id']
   assert m.tool('definitions_read',{'keys':[{'kind':'monitor_instance','id':'collection'}]})['records']
   m.close();clients=[];e.stop();provision(db,temp,'author',full,expected=1,enabled=False);e.start();m=MCP(e.port,credential);clients.append(m)
   assert m.tool('definitions_list',{},False)['error']=='unauthenticated'
   checks.append('restart persistence and operator credential revocation')
   print(json.dumps({'passed':True,'checks':checks}))
  finally:
   for m in clients:m.close()
   if proxy:proxy.terminate();proxy.wait(timeout=10)
   e.stop();fixture.close()
if __name__=='__main__':main()
