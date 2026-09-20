#!/usr/bin/env python3
"""Real process restart and response-loss checks against the durable Axum service."""
import json,os,pathlib,socket,subprocess,tempfile,urllib.request,sys,time,threading
BIN=sys.argv[1] if len(sys.argv)>1 else 'engine/target/debug/talia-engine'
def wire(x):return {'version':1,'value':['number',x]}
with tempfile.TemporaryDirectory(prefix='talia-p2-transport-') as temp:
 db=str(pathlib.Path(temp)/'engine.db');proc=None
 def start():
  global proc,port,inc
  proc=subprocess.Popen([BIN,db,'0','--seed'],stdout=subprocess.PIPE,text=True)
  info=json.loads(proc.stdout.readline());port=info['port'];inc=info['incarnation']
 def rpc(op,args=None,incarnation=None,epoch=1):
  data=json.dumps(dict(version=1,client='test-client',epoch=epoch,incarnation=inc if incarnation is None else incarnation,op=op,args=args or {})).encode()
  with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/engine',data=data,headers={'Content-Type':'application/json'}),timeout=8) as r:return json.load(r)
 def ok(op,args=None,**kw):
  r=rpc(op,args,**kw);assert 'error' not in r,r;return r['value']
 try:
  start();initial=ok('hello');assert initial['values'][0]['value']==wire(62)
  duplicate=subprocess.run([BIN,db,'0'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5);assert duplicate.returncode!=0
  assert ok('subscribe',{'id':'value'})['values'][0]['id']=='value'
  out=ok('write',{'id':'value','actionId':'write-1','expected':1,'value':wire('Infinity')});assert out['status']=='complete'
  assert ok('write',{'id':'value','actionId':'write-1','expected':1,'value':wire('Infinity')})==out
  assert 'error' in rpc('write',{'id':'value','actionId':'write-1','expected':1,'value':wire(9)})
  before=ok('snapshot');assert before['values'][0]['revision']==2
  old=inc;proc.kill();proc.wait();start();assert old!=inc
  assert 'error' in rpc('snapshot',incarnation=old)
  after=ok('hello');assert after==before
  assert ok('status',{'actionId':'write-1'})==out
  assert ok('status',{'actionId':'not-recorded'})['status']=='unknown'
  # Send a mutation and drop its response; reconcile by stable action identity.
  body=json.dumps(dict(version=1,client='test-client',epoch=1,incarnation=inc,op='write',args={'id':'value','actionId':'lost-response','expected':2,'value':wire('NaN')})).encode()
  with socket.create_connection(('127.0.0.1',port)) as s:s.sendall(f'POST /engine HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n'.encode()+body);s.recv(1) # discard the response before reading its outcome
  assert ok('status',{'actionId':'lost-response'})['status']=='complete'
  assert ok('snapshot')['values'][0]['revision']==3
  definition=dict(id='setter',version=1,source="{get(){return 0},async set(c,v){await c.write('value',-Infinity);await c.sleep(3000);return v}}",kind='computed',value_schema='any',state_schema='any',dependencies=['value'],read_policy='shared')
  ok('define',{'definition':definition,'expected':0})
  undefined={'version':1,'value':['undefined']}
  instance=dict(id='setter',definition='setter',params=undefined,state=undefined,value=undefined,has_value=False,timestamp=0,quality='unknown',revision=1,generation=1,history_count=0,history_age_ms=0)
  ok('create',{'instance':instance})
  def accepted_effect():
   try:rpc('set',{'id':'setter','actionId':'interrupted-effect','value':wire(7)})
   except Exception:pass
  task=threading.Thread(target=accepted_effect);task.start()
  deadline=time.monotonic()+5
  while time.monotonic()<deadline:
   if ok('read',{'id':'value'})['value']==wire('-Infinity'):break
   time.sleep(.02)
  else:raise AssertionError('effect did not run')
  proc.kill();proc.wait();task.join();start()
  assert ok('status',{'actionId':'interrupted-effect'})['status']=='unknown'
  assert ok('set',{'id':'setter','actionId':'interrupted-effect','value':wire(7)})['status']=='unknown'
  assert ok('read',{'id':'value'})['value']==wire('-Infinity')
  ok('hello',epoch=2);assert 'error' in rpc('snapshot',epoch=1)
  ok('unsubscribe',{'id':'value'},epoch=2)
  print(json.dumps({'passed':True,'checks':['single owner','durable writes','duplicate identity','argument conflict','server incarnation','restart snapshot','original age/quality','response loss reconciliation','unknown stays unknown','stale client epoch','subscription lifecycle','crash after effect remains unknown without replay']}))
 finally:
  if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=5)
