#!/usr/bin/env python3
"""Authenticated host registration over the real HTTP adapter and process restart."""
import json,pathlib,secrets,subprocess,sys,tempfile,urllib.request
binary=sys.argv[1] if len(sys.argv)>1 else 'engine/target/debug/talia-engine'
with tempfile.TemporaryDirectory(prefix='talia-clients-http-') as temp:
 proc=None;credential=secrets.token_hex(32);owner=secrets.token_hex(32)
 def start():
  global proc,port
  proc=subprocess.Popen([binary,str(pathlib.Path(temp)/'engine.db'),'0'],stdout=subprocess.PIPE,text=True);port=json.loads(proc.stdout.readline())['port']
 def request(body,token=credential):
  headers={'Content-Type':'application/json','Authorization':'Bearer '+token}
  with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/clients',data=json.dumps(body).encode(),headers=headers),timeout=5) as r:return json.load(r)
 def ok(body):
  r=request(body);assert 'error' not in r,r;return r['value']
 try:
  start();registration={'op':'register','name':'Kitchen','platform':'web'};client=ok(registration);assert ok(registration)==client
  assert request({'op':'status'},'')['error']=='unauthenticated'
  assert request({'op':'status'},secrets.token_hex(32))['error']=='unauthenticated'
  assert request({'op':'status','principal':'root'})['error']=='invalid_input'
  report=dict(dashboardId='monitor',packageRevision='v1',lifecycle='active',foreground=True,dirty=False,editRevision=0,updateAvailable=False)
  address=dict(slot='tab',owner=owner,live='one',epoch=1)
  first=ok(dict(op='connect',**address,report=report));ok(dict(op='report',**address,sequence=1,report={**report,'dirty':True,'editRevision':1}))
  assert request(dict(op='report',**address,sequence=2,report=report))['error']=='conflict'
  assert request(dict(op='connect',**{**address,'epoch':2},report=report))['error']=='conflict'
  assert request(dict(op='report',**{**address,'owner':secrets.token_hex(32)},sequence=3,report=report))['error']=='forbidden'
  ok(dict(op='connect',**{**address,'epoch':2,'live':'two'},previous='one',report=report))
  assert request(dict(op='report',**address,sequence=99,report=report))['error']=='stale_instance'
  assert request(dict(op='connect',**{**address,'epoch':3},previous='two',report=report))['error']=='stale_instance'
  public=ok({'op':'status'});assert credential not in json.dumps(public) and owner not in json.dumps(public)
  proc.kill();proc.wait();start();assert not ok({'op':'status'})['slots'][0]['connected']
  ok(dict(op='connect',**{**address,'epoch':3,'live':'two'},report=report));assert ok({'op':'status'})['slots'][0]['connected']
  print(json.dumps({'passed':True,'checks':['idempotent enrollment','authentication','strict schema','dirty state cannot regress','reconnect cannot clear dirty state','slot ownership','obsolete reports rejected','retired IDs cannot return','credential redaction','restart disconnects until authenticated reconnect']}))
 finally:
  if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=5)
