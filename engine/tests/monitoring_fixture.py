#!/usr/bin/env python3
"""Owned fixture harness shared by browser/native monitoring qualification."""
import json,os,pathlib,subprocess,sys,tempfile
import monitoring
class Harness:
 def __init__(self,binary):
  monitoring.BIN=binary;self.temp=tempfile.TemporaryDirectory(prefix='talia-p3-clients-');self.fixture=monitoring.Fixture();self.engine=monitoring.EngineProcess(pathlib.Path(self.temp.name)/'engine.db');self.server=None
  try:
   self.engine.start();self.engine.seed(monitoring.config(self.fixture.url))
   self.server=subprocess.Popen(['python3','dashboard/serve.py','0'],env={**os.environ,'TALIA_ENGINE_DB':self.engine.db,'TALIA_ENGINE_PORT':str(self.engine.port)},stdout=subprocess.PIPE,text=True);self.port=json.loads(self.server.stdout.readline())['port']
  except: self.close();raise
 def close(self):
  if self.server and self.server.poll() is None:self.server.terminate();self.server.wait(timeout=5)
  self.engine.stop();self.fixture.close();self.temp.cleanup()
 def command(self,command):
  if command['op']=='disk':self.fixture.disk=command['value']
  elif command['op']=='stop':self.engine.stop(kill=True)
  elif command['op']=='restart':self.engine.stop(kill=True);self.engine.start()
  return {'probes':self.fixture.probes,'calls':self.fixture.calls,'port':self.port}
if __name__=='__main__':
 h=Harness(sys.argv[1]);print(json.dumps({'port':h.port}),flush=True)
 try:
  for line in sys.stdin:print(json.dumps(h.command(json.loads(line))),flush=True)
 finally:h.close()
