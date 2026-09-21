#!/usr/bin/env python3
"""Real MCP adapter and both native dashboard hosts, with isolated credentials/database."""
import pathlib,tempfile,subprocess,json,os,sys
from mcp_authoring import provision,grant,MCP
from monitoring import EngineProcess
import mcp_authoring
with tempfile.TemporaryDirectory(prefix='talia-live-') as temp:
 db=pathlib.Path(temp)/'engine.db'
 live={'family':'live','actions':['list','inspect','execute','reload','audit'],'scope':{'kind':'all'}}
 engine={'family':'engine','actions':['read','write','audit'],'scope':{'kind':'all'}}
 token=provision(db,temp,'live',[live,engine,grant(['list','read'])])
 denied=provision(db,temp,'denied',[{'family':'live','actions':['list','inspect','execute','reload'],'scope':{'kind':'all'}}])
 e=EngineProcess(db);proxy=None
 try:
  e.start();proxy=subprocess.Popen(['python3','dashboard/serve.py','0'],env={**os.environ,'TALIA_ENGINE_DB':str(db),'TALIA_ENGINE_PORT':str(e.port)},stdout=subprocess.PIPE,text=True);port=json.loads(proxy.stdout.readline())['port']
  subprocess.run(['node','engine/tests/mcp-live-web.mjs',str(e.port),str(port),str(token),str(denied)],check=True)
  if '--android' in sys.argv:subprocess.run(['python3','engine/tests/mcp_live_android.py',str(e.port),str(port),str(token)],check=True)
  m=MCP(e.port,token);before=m.tool('operation_status',{'requestId':'reload-ack'});m.close();e.stop();e.start();m=MCP(e.port,token)
  try:
   assert m.tool('operation_status',{'requestId':'reload-ack'})==before
   assert all(not slot['connected'] for c in m.tool('clients_list',{})['clients'] for slot in c['slots'])
   print(json.dumps({'passed':True,'checks':['live reload outcome survives server restart','restart disconnects live targets without command replay']}))
  finally:m.close()
 finally:
  if proxy:proxy.terminate();proxy.wait(timeout=10)
  e.stop()
