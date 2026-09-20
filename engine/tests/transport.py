#!/usr/bin/env python3
"""Real process restart and response-loss checks against the durable Axum service."""
import json,os,pathlib,socket,subprocess,tempfile,urllib.request,sys,time
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
  ok('hello',epoch=2);assert 'error' in rpc('snapshot',epoch=1)
  ok('unsubscribe',{'id':'value'},epoch=2)
  print(json.dumps({'passed':True,'checks':['single owner','durable writes','duplicate identity','argument conflict','server incarnation','restart snapshot','original age/quality','response loss reconciliation','unknown stays unknown','stale client epoch','subscription lifecycle']}))
 finally:
  if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=5)
